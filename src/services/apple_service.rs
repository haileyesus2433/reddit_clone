use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use oauth2::{
    AccessToken, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl,
    ResponseType, Scope, TokenResponse, TokenUrl, basic::BasicClient, url,
};
use serde::{Deserialize, Serialize};
use std::error::Error;

type AppleClient = oauth2::Client<
    oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
    oauth2::StandardTokenResponse<oauth2::EmptyExtraTokenFields, oauth2::basic::BasicTokenType>,
    oauth2::StandardTokenIntrospectionResponse<
        oauth2::EmptyExtraTokenFields,
        oauth2::basic::BasicTokenType,
    >,
    oauth2::StandardRevocableToken,
    oauth2::StandardErrorResponse<oauth2::RevocationErrorResponseType>,
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

#[derive(Debug, Serialize, Deserialize)]
struct AppleJWTClaims {
    iss: String, // Team ID
    iat: i64,
    exp: i64,
    aud: String,
    sub: String, // Client ID
}

#[derive(Debug, Deserialize)]
pub struct AppleIdToken {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<String>,
    pub name: Option<serde_json::Value>,
    pub aud: String,
    pub iss: String,
    pub exp: i64,
    pub iat: i64,
}

#[derive(Debug, Deserialize)]
struct ApplePublicKey {
    kty: String,
    kid: String,
    #[serde(rename = "use")]
    key_use: String,
    alg: String,
    n: String,
    e: String,
}

#[derive(Debug, Deserialize)]
struct ApplePublicKeys {
    keys: Vec<ApplePublicKey>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppleUserData {
    pub user_id: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

pub struct AppleOAuthService {
    client: AppleClient,
    client_id: String,
    team_id: String,
    key_id: String,
    private_key: String,
    http_client: reqwest::Client,
}

impl AppleOAuthService {
    pub fn new(
        client_id: &str,
        team_id: &str,
        key_id: &str,
        private_key: &str,
        redirect_uri: &str,
    ) -> Result<Self, Box<dyn Error>> {
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let client = BasicClient::new(ClientId::new(client_id.to_string()))
            .set_auth_uri(AuthUrl::new(
                "https://appleid.apple.com/auth/authorize".to_string(),
            )?)
            .set_token_uri(TokenUrl::new(
                "https://appleid.apple.com/auth/token".to_string(),
            )?)
            .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string())?);

        Ok(Self {
            client,
            client_id: client_id.to_string(),
            team_id: team_id.to_string(),
            key_id: key_id.to_string(),
            private_key: private_key.to_string(),
            http_client,
        })
    }

    fn create_client_secret(&self) -> Result<String, Box<dyn Error>> {
        let encoding_key = EncodingKey::from_ec_pem(self.private_key.as_bytes())?;

        let now = Utc::now();
        let exp = now + Duration::minutes(60);

        let claims = AppleJWTClaims {
            iss: self.team_id.clone(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            aud: "https://appleid.apple.com".to_string(),
            sub: self.client_id.clone(),
        };

        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());

        let token = encode(&header, &claims, &encoding_key)?;
        Ok(token)
    }

    pub fn get_authorization_url(&self) -> (url::Url, CsrfToken) {
        self.client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("name".to_string()))
            .add_scope(Scope::new("email".to_string()))
            .set_response_type(&ResponseType::new("code".to_string()))
            .url()
    }

    pub async fn exchange_code_for_token(&self, code: &str) -> Result<AccessToken, Box<dyn Error>> {
        let client_secret = self.create_client_secret()?;

        let client_with_secret = self
            .client
            .clone()
            .set_client_secret(ClientSecret::new(client_secret));

        let token_result = client_with_secret
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(&self.http_client)
            .await?;

        Ok(token_result.access_token().clone())
    }

    async fn fetch_apple_public_keys(&self) -> Result<ApplePublicKeys, Box<dyn Error>> {
        let response = self
            .http_client
            .get("https://appleid.apple.com/auth/keys")
            .send()
            .await?;

        let keys: ApplePublicKeys = response.json().await?;
        Ok(keys)
    }

    pub async fn verify_apple_id_token(
        &self,
        id_token: &str,
    ) -> Result<AppleIdToken, Box<dyn Error>> {
        let header = jsonwebtoken::decode_header(id_token)?;
        let kid = header.kid.ok_or("Missing kid in token header")?;

        let public_keys = self.fetch_apple_public_keys().await?;
        let public_key = public_keys
            .keys
            .iter()
            .find(|key| key.kid == kid)
            .ok_or("No matching public key found")?;

        let decoding_key = DecodingKey::from_rsa_components(&public_key.n, &public_key.e)?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.client_id]);
        validation.set_issuer(&["https://appleid.apple.com"]);

        let token_data = decode::<AppleIdToken>(id_token, &decoding_key, &validation)?;
        Ok(token_data.claims)
    }

    pub async fn get_user_data(
        &self,
        id_token: Option<String>,
    ) -> Result<AppleUserData, Box<dyn Error>> {
        if let Some(id_token) = id_token {
            let apple_id_token = self.verify_apple_id_token(&id_token).await?;

            let user_data = AppleUserData {
                user_id: apple_id_token.sub,
                email: apple_id_token.email,
                email_verified: apple_id_token
                    .email_verified
                    .map(|v| v == "true")
                    .unwrap_or(false),
                first_name: None,
                last_name: None,
            };

            Ok(user_data)
        } else {
            Err("No ID token received from Apple".into())
        }
    }
}
