# Fig

A command line tool that provides utility functions for developing at Figure.

## Installation

```
git clone git@github.com:scirner22/figure-cli.git
cd figure-cli
cargo install --path .
```

## Usage

See all available commands

```
fig help
```

Check all required dependencies

```
fig doctor
```

Install a fig config file that contains examples to help with setup. This will create
`.fig/default.toml`. This default configuration is perfect for repos with a single application deployment.
The default.toml can be copied to .fig/subproject1.toml to configure an application by name. When you
want to reference something other than default in a fig command, you must use the optional global
parameter of `--config` or `-c`. Using multiples of this scheme with different names allows you to
have any number of referenceable configurations.

```
fig init
```

Drop into a psql shell in the test environment

```
fig psql -e test
```

## Towards 1.0

### The goal of 1.0 is to be feature compatible with everything I currently have in bash scripts.

- [x] psql command - local env
- [x] psql command - test env
- [x] psql command - prod env
- [x] doctor command
- [x] don't process commands unless config file is in .gitignore
- [x] init command - basic stub with examples
- [ ] release as 0.2.0
- [ ] subproject enabled
- [ ] init command - generate the majority of the toml config file based on parsing the project
- [ ] exec command?
- [ ] log command?
- [ ] port-forward command?
