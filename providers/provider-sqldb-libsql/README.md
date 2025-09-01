# 🪶 SQL Database libSQL Provider

This capability provider implements the [`wasmcloud:sqldb-libsql`][wasmcloud-sqldb-libsql-wit] WIT package, which enables SQL-driven database interaction with a [libSQL][libsql] database cluster.

This provider handles concurrent component connections, and components which are linked to it should specify configuration at link time (see [the named configuration settings section](#-named-configuration-settings) for more details.

Want to read all the functionality included the interface? [Start from `provider.wit`][provider-wit] to read what this provider can do, and work your way to  [`query.wit`][query-wit] and [`types.wit`][types-wit].

Note that connections are local to a single provider, so multiple providers running on the same lattice will _not_ share connections automatically.

[libsql]: https://docs.turso.tech/libsql
[wasmcloud-sqldb-libsql-wit]: https://github.com/jtakakura/wasmcloud-provider-sqldb-libsql/tree/main/wit
[provider-wit]: https://github.com/jtakakura/wasmcloud-provider-sqldb-libsql/blob/main/wit/provider.wit
[query-wit]: https://github.com/jtakakura/wasmcloud-provider-sqldb-libsql/blob/main/wit/query.wit
[types-wit]: https://github.com/jtakakura/wasmcloud-provider-sqldb-libsql/blob/main/wit/types.wit

## 📑 Named configuration Settings

As connection details are considered sensitive information, they should be specified via named configuration to the provider, and _specified_ via link definitions.
WADM files should not be checked into source control containing secrets.

New named configuration can be specified by using `wash config put`.

| Property                | Example     | Description                                                                                                                                                         |
| ----------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `LIBSQL_URL`         | `http://localhost:8080` | Remote libSQL Database URL                                                                                                                                           |
| `LIBSQL_NAMESPACE`     | `libsql`  | Remote libSQL Database Namespace                                                                                                                                           |
| `LIBSQL_POOL_SIZE`    | `12`        | Maximum size of the connection pool (configures [max_size](https://docs.rs/deadpool-libsql/0.1.0/deadpool_libsql/struct.PoolConfig.html#structfield.max_size)) |

Once named configuration with the keys above is created, it can be referenced as `target_config` for a link to this provider.

For example, the following WADM manifest fragment:

```yaml
- name: querier
  type: component
  properties:
    image: file://./build/sqldb_libsql_query_s.wasm
  traits:
    - type: spreadscaler
      properties:
        instances: 1
    - type: link
      properties:
        target: sqldb-libsql
        namespace: wasmcloud
        package: libsql
        interfaces: [execute, query]
        target_config:
          - name: default-libsql
```

The `querier` component in the snippet above specifies a link to a `sqldb-libsql` target, with `target_config` that is only specifies `name` (no `properties`).

> [!WARNING]
> While `LIBSQL_AUTH_TOKEN` can be specified as named configuration, it should be specified as a secret.
>
> In a future version, this will be required.

## 🔐 Secret Settings

While most values can be specified via named configuration, sensitive values like the `LIBSQL_AUTH_TOKEN` should be specified via _secrets_.

New secrets be specified by using `wash secrets put`.

| Property            | Example    | Description               |
| ------------------- | ---------- | ------------------------- |
| `LIBSQL_AUTH_TOKEN` | `xxxxx.yyyyy.zzzzz` | Remote libSQL Database Auth Token |

Once a secret has been created, it can be referenced in the link to the provider.

For example, the following WADM manifest fragment:

```yaml
- name: querier
  type: component
  properties:
    image: file://./build/sqldb_libsql_query_s.wasm
  traits:
    - type: spreadscaler
      properties:
        instances: 1
    - type: link
      properties:
        target: sqldb-libsql
        namespace: wasmcloud
        package: libsql
        interfaces: [execute, query]
        target_secrets:
          - name: default-libsql-secrets
```

The `querier` component in the snippet above specifies a link to a `sqldb-libsql` target, with `target_config` that is only specifies `name` (no `properties`).
