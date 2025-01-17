use anyhow::{anyhow, Result};
use async_std::fs::File;
use async_std::prelude::*;
use async_trait::async_trait;
use oci_distribution::client::{Client, ClientConfig, ClientProtocol};
use oci_distribution::secrets::RegistryAuth;
use oci_distribution::Reference;
use std::{path::Path, str::FromStr};
use tokio_compat_02::FutureExt;
use url::Url;

use crate::fetcher::Fetcher;
use crate::registry::config::{DockerConfig, RegistryAuth as OwnRegistryAuth};
use crate::sources::Sources;

pub mod config;

// Struct used to reference a WASM module that is hosted on an OCI registry
pub(crate) struct Registry {
    // full path to the WASM module
    destination: String,
    // url of the remote WASM module
    wasm_url: String,
    // host of the remote WASM module
    wasm_url_host: String,
    // configuration resembling `~/.docker/config.json` to some extent
    docker_config: Option<DockerConfig>,
}

impl Registry {
    pub(crate) fn new(
        url: Url,
        docker_config: Option<DockerConfig>,
        download_dir: &str,
    ) -> Result<Registry> {
        match url.path().rsplit('/').next() {
            Some(image_ref) => {
                let wasm_url = url.to_string();
                let dest = Path::new(download_dir).join(image_ref);

                Ok(Registry {
                    destination: String::from(
                        dest.to_str()
                            .ok_or_else(|| anyhow!("Cannot build final path destination"))?,
                    ),
                    wasm_url: wasm_url
                        .strip_prefix("registry://")
                        .map_or(Default::default(), |url| url.into()),
                    wasm_url_host: url
                        .host()
                        .map_or(Default::default(), |host| format!("{}", host)),
                    docker_config,
                })
            }
            _ => Err(anyhow!(
                "Cannot infer name of the remote file by looking at {}",
                url.path()
            )),
        }
    }

    fn client(&self, client_protocol: ClientProtocol) -> Client {
        Client::new(ClientConfig {
            protocol: client_protocol,
        })
    }

    fn auth(&self, registry: &Reference) -> RegistryAuth {
        self.docker_config
            .as_ref()
            .and_then(|docker_config| {
                docker_config.auths.get(registry.registry()).map(|auth| {
                    let OwnRegistryAuth::BasicAuth(username, password) = auth;
                    RegistryAuth::Basic(
                        String::from_utf8(username.clone()).unwrap_or_default(),
                        String::from_utf8(password.clone()).unwrap_or_default(),
                    )
                })
            })
            .unwrap_or(RegistryAuth::Anonymous)
    }
}

impl Registry {
    async fn do_fetch(
        &self,
        mut client: Client,
        reference: &Reference,
        registry_auth: &RegistryAuth,
    ) -> Result<Vec<u8>> {
        client
            .pull(
                reference,
                registry_auth,
                vec!["application/vnd.wasm.content.layer.v1+wasm"],
            )
            // We need to call to `compat()` provided by the `tokio-compat-02` crate
            // so that the Future returned by the `oci-distribution` crate can be
            // executed by a newer Tokio runtime.
            .compat()
            .await?
            .layers
            .into_iter()
            .next()
            .map(|layer| layer.data)
            .ok_or_else(|| anyhow!("could not download WASM module"))
    }

    async fn fetch_tls(
        &self,
        reference: &Reference,
        registry_auth: &RegistryAuth,
    ) -> Result<Vec<u8>> {
        let https_client = self.client(ClientProtocol::Https);
        self.do_fetch(https_client, reference, registry_auth).await
    }

    async fn fetch_plain(
        &self,
        reference: &Reference,
        registry_auth: &RegistryAuth,
    ) -> Result<Vec<u8>> {
        let http_client = self.client(ClientProtocol::Http);
        self.do_fetch(http_client, reference, registry_auth).await
    }
}

#[async_trait]
impl Fetcher for Registry {
    async fn fetch(&self, sources: &Sources) -> Result<String> {
        let reference = Reference::from_str(self.wasm_url.as_str())?;
        let registry_auth = self.auth(&reference);

        let mut image_content = self.fetch_tls(&reference, &registry_auth).await;
        if let Err(err) = image_content {
            if !sources.is_insecure_source(&self.wasm_url_host) {
                return Err(anyhow!(
                    "could not download Wasm module: {}; host is not an insecure source",
                    err
                ));
            }
            image_content = self.fetch_plain(&reference, &registry_auth).await;
        }

        match image_content {
            Ok(image_content) => {
                let mut file = File::create(self.destination.clone()).await?;
                file.write_all(&image_content[..]).await?;

                Ok(self.destination.clone())
            }
            Err(err) => Err(anyhow!("could not download Wasm module: {}", err)),
        }
    }
}
