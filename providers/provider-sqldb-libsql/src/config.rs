use tracing::warn;
use wasmcloud_provider_sdk::{core::secrets::SecretValue, LinkConfig};

/// Creation options for a libSQL connection
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectionCreateOptions {
    /// URL of the libSQL cluster to connect to
    pub url: String,
    /// Auth token used when accessing the libSQL cluster
    pub auth_token: String,
    /// Optional namespace to use for the connection
    pub namespace: Option<String>,
    /// Optional connection pool size
    pub pool_size: Option<usize>,
}

impl From<ConnectionCreateOptions> for deadpool_libsql::Config {
    fn from(opts: ConnectionCreateOptions) -> Self {
        let database = deadpool_libsql::config::Database::Remote(deadpool_libsql::config::Remote {
            url: opts.url,
            auth_token: opts.auth_token,
            namespace: opts.namespace,
            remote_encryption: None,
        });

        let mut config = deadpool_libsql::Config::new(database);
        if let Some(pool_size) = opts.pool_size {
            config.pool = deadpool_libsql::PoolConfig {
                max_size: pool_size,
                ..deadpool_libsql::PoolConfig::default()
            };
        }
        config
    }
}

/// Parse the options for libSQL configuration from a [`HashMap`], with a given prefix to the keys
///
/// For example given a prefix like `EXAMPLE_`, and a Hashmap that contains an entry like ("EXAMPLE_HOST", "localhost"),
/// the parsed [`ConnectionCreateOptions`] would contain "localhost" as the host.
pub(crate) fn extract_prefixed_conn_config(
    prefix: &str,
    link_config: &LinkConfig,
) -> Option<ConnectionCreateOptions> {
    let LinkConfig {
        config, secrets, ..
    } = link_config;

    let keys = [
        format!("{prefix}URL"),
        format!("{prefix}AUTH_TOKEN"),
        format!("{prefix}NAMESPACE"),
        format!("{prefix}POOL_SIZE"),
    ];
    match keys
        .iter()
        .map(|k| {
            // Prefer fetching from secrets, but fall back to config if not found
            match (secrets.get(k).and_then(SecretValue::as_string), config.get(k)) {
                (Some(s), Some(_)) => {
                    warn!("secret value [{k}] was found in secrets, but also exists in config. The value in secrets will be used.");
                    Some(s)
                }
                (Some(s), _) => Some(s),
                // Offer a warning for the auth token, but other values are fine to be in config
                (None, Some(c)) if k == &format!("{prefix}AUTH_TOKEN") => {
                    warn!("secret value [{k}] was not found in secrets, but exists in config. Prefer using secrets for sensitive values.");
                    Some(c.as_str())
                }
                (None, Some(c)) => {
                    Some(c.as_str())
                }
                (_, None) => None,
            }
        })
        .collect::<Vec<Option<&str>>>()[..]
    {
        [Some(url), auth_token, namespace, pool_size] =>
        {
            let pool_size = pool_size.and_then(|pool_size| {
                pool_size.parse::<usize>().ok().or_else(|| {
                    warn!("invalid pool size value [{pool_size}], using default");
                    None
                })
            });

            Some(ConnectionCreateOptions {
                url: url.to_string(),
                auth_token: auth_token.map(|s| s.to_string()).unwrap_or_default(),
                namespace: namespace.map(|s| s.to_string()),
                pool_size,
            })
        }
        _ => {
            warn!("failed to find required keys in configuration: [{:?}]", keys);
            None
        }
    }
}
