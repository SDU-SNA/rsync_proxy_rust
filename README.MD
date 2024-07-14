# RSYNC-PROXY

A simple rsync proxy server that can be used to balance rsync traffic across multiple rsync servers.

Author: [futz12](https://www.github.com/futz12)

## Usage

### Build

```bash
cargo build --release
```

### Run

```bash
./target/release/rsync-proxy
```

### WebAPI

#### Get current config

```bash
curl http://localhost:3030/config | jq
```

#### Update config

```bash
curl -X POST http://localhost:3030/update -d '[
{
     "module_name": "archlinux",
     "upstreams": [
          "127.0.0.1:5000"
     ]
},
{
     "module_name": "lxc-images",
     "upstreams": [
          "127.0.0.1:5000"
     ]
}]' -H "Content-Type: application/json"
```