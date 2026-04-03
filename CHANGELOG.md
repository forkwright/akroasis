# Changelog

## [0.1.8](https://github.com/forkwright/akroasis/compare/v0.1.7...v0.1.8) (2026-04-03)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([7cf413c](https://github.com/forkwright/akroasis/commit/7cf413c429a074794af5e3353167ba8145cee035))

## [0.1.7](https://github.com/forkwright/akroasis/compare/v0.1.6...v0.1.7) (2026-03-27)


### Features

* **kerykeion:** gateway bridge and mesh collector wiring ([#71](https://github.com/forkwright/akroasis/issues/71)) ([8da4fc1](https://github.com/forkwright/akroasis/commit/8da4fc1eb6c69c802643fa24514ec95a4a42b4fa))
* **kerykeion:** node discovery, mesh topology graph, and peer tracking ([#68](https://github.com/forkwright/akroasis/issues/68)) ([ce27b16](https://github.com/forkwright/akroasis/commit/ce27b16cb82f4c1d1eefe2d831d0130180eb7036))

## [0.1.6](https://github.com/forkwright/akroasis/compare/v0.1.5...v0.1.6) (2026-03-24)


### Bug Fixes

* add [graph] section to deny.toml for cargo-deny 0.19 compatibility ([86af2d9](https://github.com/forkwright/akroasis/commit/86af2d9a004bceb49cceef66cfaf6b4513d0e2d6))

## [0.1.5](https://github.com/forkwright/akroasis/compare/v0.1.4...v0.1.5) (2026-03-22)


### Features

* **kerykeion:** gateway bridge, MQTT parsing, collector run loop, and mesh CLI ([#62](https://github.com/forkwright/akroasis/issues/62)) ([86b5eb2](https://github.com/forkwright/akroasis/commit/86b5eb24935529724fb8e6c07d5ce671177454a1))
* **kerykeion:** message routing, store-and-forward, and delivery tracking ([#61](https://github.com/forkwright/akroasis/issues/61)) ([8a7d306](https://github.com/forkwright/akroasis/commit/8a7d30615eec3c3deddf04183c1f6e8615c0150a))
* **kerykeion:** node discovery, mesh topology graph, and peer tracking ([#60](https://github.com/forkwright/akroasis/issues/60)) ([570ef34](https://github.com/forkwright/akroasis/commit/570ef34405d66c7149407b7047c69bf3f172802e))

## [0.1.4](https://github.com/forkwright/akroasis/compare/v0.1.3...v0.1.4) (2026-03-19)


### Features

* **kerykeion:** serial/TCP transport, frame codec, AES-CTR encryption, config handshake ([221f2b7](https://github.com/forkwright/akroasis/commit/221f2b7cf23441dd5c90a359ae33ad930d37cdcd))
* **kerykeion:** serial/TCP transport, frame codec, AES-CTR encryption, config handshake ([e84cebc](https://github.com/forkwright/akroasis/commit/e84cebcbf7ed303fda4a58725760710932434b0e))


### Bug Fixes

* **ci:** allow MPL-2.0 license (serialport crate) + thiserror-impl duplicate ([f1f8873](https://github.com/forkwright/akroasis/commit/f1f887353d3c2d201525f2af247c4d20f568af10))
* rustdoc private item links + cargo-deny thiserror duplicate ([c2676c5](https://github.com/forkwright/akroasis/commit/c2676c5e9b9762f8e3c45a674fdaee8f62a9ab4f))

## [0.1.3](https://github.com/forkwright/akroasis/compare/v0.1.2...v0.1.3) (2026-03-19)


### Features

* **kerykeion:** crate scaffold with mesh types, config, and protobuf codegen ([#52](https://github.com/forkwright/akroasis/issues/52)) ([23da372](https://github.com/forkwright/akroasis/commit/23da372dbde8145f00f3f6f2a637e5f1530a1ec2))

## [0.1.2](https://github.com/forkwright/akroasis/compare/v0.1.1...v0.1.2) (2026-03-18)


### Bug Fixes

* **ci:** install protobuf-compiler for kerykeion prost codegen ([#53](https://github.com/forkwright/akroasis/issues/53)) ([b79da85](https://github.com/forkwright/akroasis/commit/b79da853d05495c6e3eddb5d0a769e6dd4630a6b))

## [0.1.1](https://github.com/forkwright/akroasis/compare/v0.1.0...v0.1.1) (2026-03-18)


### Features

* **akroasis:** radio CLI — read, program, export, import, detect ([#21](https://github.com/forkwright/akroasis/issues/21)) ([4f853fc](https://github.com/forkwright/akroasis/commit/4f853fc3878d5e66d6196f56bd632abf42792437))
* expand to 17-crate architecture ([f68ab25](https://github.com/forkwright/akroasis/commit/f68ab256ec9de7c3efe070d14d095a1681654ab1))
* **koinon:** hardware asset registry with USB device identification ([#12](https://github.com/forkwright/akroasis/issues/12)) ([b2224d9](https://github.com/forkwright/akroasis/commit/b2224d924844d6af7d76687a8f8a8972bd93bfe2))
* **koinon:** signal model, GeoSignal, and Entity types ([#13](https://github.com/forkwright/akroasis/issues/13)) ([d4e0480](https://github.com/forkwright/akroasis/commit/d4e04807d98d855614c62aef7f034b10a0a63fc6))
* **koinon:** tamper-evident log with BLAKE3 hash chain ([#6](https://github.com/forkwright/akroasis/issues/6)) ([23fc071](https://github.com/forkwright/akroasis/commit/23fc0719192342fdcc73432d0abe08b62870ddb5))
* **koinon:** temporal baseline engine with Welford's algorithm ([#14](https://github.com/forkwright/akroasis/issues/14)) ([5dadc33](https://github.com/forkwright/akroasis/commit/5dadc332f79a86766302313bc82c95b032c58eb5))
* **koinon:** workspace scaffold with shared types + CLI skeleton ([#5](https://github.com/forkwright/akroasis/issues/5)) ([d01cdab](https://github.com/forkwright/akroasis/commit/d01cdab57fdc750f0e146cc5897ccf78931a7ef3))
* **kryphos:** add Argon2id KDF and ChaCha20-Poly1305 encryption core ([#33](https://github.com/forkwright/akroasis/issues/33)) ([c8ec08d](https://github.com/forkwright/akroasis/commit/c8ec08d0a23ccabfc15958bd941dbf83a2a33114))
* **kryphos:** add Ed25519 signing, vault storage, and tamper log integration ([#34](https://github.com/forkwright/akroasis/issues/34)) ([9b286b5](https://github.com/forkwright/akroasis/commit/9b286b5d2052ebd197a82d33e46a09898db51716))
* **kryphos:** add figment provider for vault-backed config values ([#37](https://github.com/forkwright/akroasis/issues/37)) ([6f12afd](https://github.com/forkwright/akroasis/commit/6f12afd2403040867f922cfcd375763338780d5c))
* **kryphos:** add fjall-backed vault storage with advisory locking ([#35](https://github.com/forkwright/akroasis/issues/35)) ([b83eb1b](https://github.com/forkwright/akroasis/commit/b83eb1b0968ceb19ba2398aba08923b761137eba))
* **kryphos:** add key lifecycle — rotation, revocation, and audit ([#36](https://github.com/forkwright/akroasis/issues/36)) ([0730115](https://github.com/forkwright/akroasis/commit/0730115d93f54173c2afa62678775eed07ce52e3))
* **kryphos:** add vault CLI — init, add, list, get, rotate, revoke, identity ([#38](https://github.com/forkwright/akroasis/issues/38)) ([df3ac1a](https://github.com/forkwright/akroasis/commit/df3ac1a67ea79d955079dc8d233d4adfbf709be5))
* **kryphos:** scaffold crate with vault data model and key types ([#32](https://github.com/forkwright/akroasis/issues/32)) ([5ead4b2](https://github.com/forkwright/akroasis/commit/5ead4b235f655665cad99c7e364a940a45ce05bc))
* **syntonia:** BF-F8HP + UV-5RM Plus variant support ([#19](https://github.com/forkwright/akroasis/issues/19)) ([4d44e09](https://github.com/forkwright/akroasis/commit/4d44e09a3b0ebb1898dabbf6aadb7cd3108b99d2))
* **syntonia:** channel data model + frequency plan + validation ([#15](https://github.com/forkwright/akroasis/issues/15)) ([dd21fd6](https://github.com/forkwright/akroasis/commit/dd21fd63c167afa70a8903e776607566170a6c82))
* **syntonia:** CHIRP .img and .csv import/export for UV-5R ([#20](https://github.com/forkwright/akroasis/issues/20)) ([61bba3d](https://github.com/forkwright/akroasis/commit/61bba3d4f8a2359679794f2a7a269c6cdb9f79db))
* **syntonia:** USB hardware detection + radio auto-detect ([#16](https://github.com/forkwright/akroasis/issues/16)) ([37de389](https://github.com/forkwright/akroasis/commit/37de389dfaae054ddc5912e6b931cafef0bb9227))
* **syntonia:** UV-5R EEPROM clone protocol ([#18](https://github.com/forkwright/akroasis/issues/18)) ([9cf06be](https://github.com/forkwright/akroasis/commit/9cf06be1ca631a34d44f9a207dcc935363cbcbc7))
* **syntonia:** UV-5R EEPROM memory map + channel codec ([#17](https://github.com/forkwright/akroasis/issues/17)) ([1a3deea](https://github.com/forkwright/akroasis/commit/1a3deeae82de90c15b0c3851dc2cb35539117c91))


### Bug Fixes

* **akroasis:** use DcsCode::as_code() not nonexistent .code() method ([1cf71a8](https://github.com/forkwright/akroasis/commit/1cf71a8369e614e6c07393d90cd9bb8f23945004))
* **ci:** update deny.toml — BSD-3-Clause license, advisory ignore, warn on duplicates ([4744db4](https://github.com/forkwright/akroasis/commit/4744db4d8e34dbd05e5b31229df00a93b639b728))
* **ci:** use akroasis-specific binary and features in rust.yml ([#42](https://github.com/forkwright/akroasis/issues/42)) ([154dcb5](https://github.com/forkwright/akroasis/commit/154dcb57377ae40a9d058017459e8f3d23b9f201))
