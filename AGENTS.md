# AGENTS.md

Guidance for autonomous agents working in this repository.

## Remote infrastructure

Minecraft servers run as Docker containers on a remote host (two currently running; a
third, ephemeral 1.21.40 validation container was used once and is now stopped/removed —
see table):

- **Host:** `ai-assistant-01.longbranch.lwalker.me`
- **SSH:** `ssh -l luser ai-assistant-01.longbranch.lwalker.me` (key-based; the SSH agent on
  the local Windows machine has the keys loaded). `root` login is disabled — use
  `sudo docker ...` for container management.
- **Docker:** all management goes through `sudo docker` on that host (there is no
  docker socket exposed locally).

### The servers

| Container   | Edition  | Image                           | Server version  | Host port | Data volume |
|-------------|----------|---------------------------------|-----------------|-----------|-------------|
| `mc`        | Java     | `itzg/minecraft-server`          | 26.2 (VANILLA)  | 25565     | `mc-data`   |
| `mc-bedrock`| Bedrock  | `itzg/minecraft-bedrock-server`  | 1.26.43.1 (BDS) | 25570 tcp | `mc-bedrock-data` |
| `mc-bedrock-12140` *(used once, now stopped/removed)* | Bedrock | `itzg/minecraft-bedrock-server` | 1.21.40.03 (BDS) | 25580 tcp | ephemeral (`mc-bedrock-12140-data`) |

> **Resource constraint:** `ai-assistant-01` should run **at most two Minecraft
> containers at once**. The 1.21.40 validation container is **ephemeral** — start it
> only for the matched-version biome-validation run (PLAN §4 "1.21.40 validation
> server"), then stop/remove it when done. It exists to validate cubiomes' ≤1.21 output
> against a matching Bedrock version; it is not a permanent server. Note: the image
> `VERSION` must be the **4-part** string `1.21.40.03` (bare `1.21.40` 404s), and
> `ALLOW_CHEATS` must be lowercase `true`.

Volume mount points (on the host): `mc-data` → `/var/lib/docker/volumes/mc-data/_data`,
`mc-bedrock-data` → `/var/lib/docker/volumes/mc-bedrock-data/_data`. In-container, the
server data (including `server.properties`) lives under `/data`. The 1.21.40
validation container used its own volume and a distinct name + port.

### Key configuration (current)

- **Bedrock (`mc-bedrock`, 1.26.43):** `allow-cheats=false` (commands are cheat-only →
  must be enabled before `/locate`), `level-seed=` empty (world uses a random seed;
  read it back with `/seed`), `online-mode=true`, `allow-list=true`.
  - **Structure params: validated.** `be-struct` placement is confirmed **100%** against
    this server (PLAN §7), so treat structure params as confirmed across the
    1.21.x–1.26.43 range.
  - **Biome params: validated (GREEN).** cubiomes matches the real BDS server's
    `/locate biome` output at **100%** on both `biome-corpus-1.21.40.json` (captured
    against this 1.26.43 server) and `biome-corpus-1.21.40.bds.json` (captured against
    the 1.21.40 validation container). An earlier "18.2% / RED gate" reading was a
    y/z argument-order bug in `cubiomes-sys/src/bridge.c` (returned `deep_dark` at
    every surface coordinate), now fixed and regression-tested. Treat biome results as
    Bedrock-accurate for the surface biomes in the corpus.
  - **BDS quirks to remember when driving via `send-command`:** strip a leading `/`
    from commands ("Unknown command: /"), and pre-1.21.100 servers reject the
    `minecraft:` biome namespace (`locate biome plains` works, `minecraft:plains`
    syntax-errors). The remote driver (`be-verify/src/remote.rs`) is version-aware via
    `biome_namespace_required`.
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
