## About

Simple replacement for [fail2ban](https://www.fail2ban.org).

## Development

This project is in early stages of development, and is not yet ready for production use. If you would like to contribute, please feel free to submit a pull request or open an issue.

### SQL Compile-time verification

```bash
# build with db
echo 'DATABASE_URL=sqlite://logban.db' >> .env
sqlx database create
sqlx migrate run
cargo sqlx prepare # saving query metadata for offline mode 

# or build with offline mode
SQLX_OFFLINE=true cargo build
```
