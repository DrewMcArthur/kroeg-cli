# kroeg

> kroeg noun \- pub \- bar

This is a repository forked from [kroeg](https://puck.moe/git/kroeg/kroeg) by [Puck Meerburg](https://puck.moe).

## how to run

1. [install rust](https://www.rust-lang.org/tools/install)
2. check that the project builds with `cargo build` 
3. install postgresql
   - create a new db with `psql postgres -c 'CREATE DATABASE kroeg;'`
   - initialize the schema with `psql kroeg -f [db.sql](https://github.com/DrewMcArthur/kroeg-cellar/blob/main/schema/db.sql)` 
4. copy `server.toml.example` to `server.toml`
5. use `cargo run --bin kroeg serve` to run the server
6. use `cargo run --bin kroeg actor name create` to create a new actor named `name`
7. run `cargo doc --no-deps` to build semi-helpful docs
8. query the running server at the address configured in `server.toml`!