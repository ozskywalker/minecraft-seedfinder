# AGENTS.md

Guidance for autonomous agents working in this repository.

## Remote infrastructure

Two Minecraft servers run as Docker containers on a remote host:

- **Host:** `ai-assistant-01.longbranch.lwalker.me`
- **SSH:** `ssh -l luser ai-assistant-01.longbranch.lwalker.me` (key-based; the SSH agent on
  the local Windows machine has the keys loaded). `root` login is disabled — use
  `sudo docker ...` for container management.
- **Docker:** all management goes through `sudo docker` on that host (there is no
  docker socket exposed locally).

### The two servers

| Container   | Edition  | Image                           | Server version  | Host port | Data volume |
|-------------|----------|---------------------------------|-----------------|-----------|-------------|
| `mc`        | Java     | `itzg/minecraft-server`          | 26.2 (VANILLA)  | 25565     | `mc-data`   |
| `mc-bedrock`| Bedrock  | `itzg/minecraft-bedrock-server`  | 1.26.43.1 (BDS) | 25570 tcp | `mc-bedrock-data` |

Volume mount points (on the host): `mc-data` → `/var/lib/docker/volumes/mc-data/_data`,
`mc-bedrock-data` → `/var/lib/docker/volumes/mc-bedrock-data/_data`. In-container, the
server data (including `server.properties`) lives under `/data`.

### Key configuration (current)

- **Bedrock:** `allow-cheats=false` (commands are cheat-only → must be enabled before
  `/locate`), `level-seed=` empty (world uses a random seed; read it back with `/seed`),
  `online-mode=true`, `allow-list=true`. Version is **1.26.43**, newer than the
  project's 1.21.x version tables — treat structure/biome params as potentially
  drifted until validated.
- **Java:** `enable-rcon=true`, RCON port **25575** (not exposed to the host — reach it
  via `docker exec`/container network), RCON password is in `server.properties`
  (`rcon.password`). Version **26.2** is far newer than cubiomes' ≤1.21 support, so it
  cannot validate cubiomes biome output.

### Management commands

```sh
ssh -l luser ai-assistant-01.longbranch.lwalker.me
sudo docker ps [-a]
sudo docker logs --tail 100 <container>
sudo docker restart <container>
sudo docker stop/start <container>
sudo docker exec <container> <cmd>          # e.g. inspect files in /data
```

### Sending commands to the servers

- **Bedrock has no RCON.** Use the image's `send-command` helper (output appears in
  `docker logs`):
  ```sh
  sudo docker exec mc-bedrock send-command "locate structure village"
  sudo docker exec mc-bedrock send-command "seed"
  ```
  Commands require cheats enabled (`allow-cheats=true` in `server.properties`, or the
  `ALLOW_CHEATS` env var on recreate).
- **Java:** use RCON (or `docker attach` if the container were started with `-it`; it
  was not).

### Editing server.properties / regenerating a world

The Bedrock server reads `server.properties` from its volume (`/data/server.properties`
in-container). To change the seed you must set `level-seed`, delete the existing world
under `/data/worlds/<level-name>`, and restart — the world is generated once at first
boot. The project's `be-verify` harness model is "one fresh world per seed", which is
what an ephemeral container with `LEVEL_SEED` achieves.

## Working conventions

- This is a Cargo workspace (see `CLAUDE.md`); `cargo test --workspace` and
  `cargo clippy --workspace --all-targets` must stay green.
- Ground-truth validation must never be faked. The project's Phase 0 gate (PLAN §6/§7)
  requires generator predictions verified against the real Bedrock server before the
  search engine is trusted. Use the real servers above; record real captured output as
  fixtures, never fabricate it.
