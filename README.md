# Fig

![crates.io](https://img.shields.io/crates/v/figcli.svg)

A command line tool that provides utility functions for developing at Figure.

## Installation

See https://rustup.rs if you don't currently have `cargo` installed.

### crates.io

```bash
$ cargo install figcli
```

### Source

```bash
$ git clone git@github.com:scirner22/figure-cli.git
$ cd figure-cli/
$ cargo install --path .
```

## Usage

See all available commands

```bash
$ figcli help  # or alternatively `figcli --help` or just `figcli`
```

Check all required dependencies

```bash
$ figcli doctor
```

Install a `figcli` config file that contains examples to help with setup. The root `figcli` config directory is
`$HOME/.config/fig` on linux and `$HOME/Library/Application Support/fig` on mac. The `figcli` config for
the directory you are currently (ex. `~/code/app-identity`) is contained in `<OS specific config root>/fig/app-identity/`
This default configuration is perfect for repos with a single application deployment.
The `default.toml` can be copied to `subproject1.toml` to configure an application by name. When you
want to reference something other than default in a `figcli` command, you must use the optional global
parameter of `--config` or `-c` (`-c subproject1`). Using multiples of this scheme with different names allows you to
have any number of referenceable configurations. Note: Once running this you can edit the configuration file
and fill in the correct values.

```bash
$ figcli config init
```

List available configurations for the current directory

```bash
$ cd src/
$ figcli config list

provenance.toml
default.toml
```

Edit the `provenance.toml` configuration file

```bash
$ figcli -c provenance config edit  # will use $EDITOR
```

Drop into a psql shell in the test environment (default configuration file)

```bash
$ figcli psql test --shell
```

Drop into a psql shell in the test environment for the non default configuration

```bash
$ figcli -c provenance psql test --shell
```

Start a local pgbouncer and print the postgresql connection string that can be used to connect
with a third party Postgres query application. Pgbouncer is used so that the username and password
do not have to be used. This provides a simple way to have a third party Postgres application
configured without having to fetch and input the ever expiring Google Cloud SQL credentials
in Vault. The `--port` flag is used so a static predefined port can be used instead of finding
a randomly available one.

```bash
$ figcli psql test --port 65432
```

## Towards 1.0

- [ ] psql command - seamless vault and devops.figure.com for credential management
- [ ] init command - generate the majority of the toml config file based on parsing the project
- [ ] exec command?
- [ ] log command?
- [ ] port-forward command?
