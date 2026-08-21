# Changelog

## [0.4.4](https://github.com/forkwright/akroasis/compare/v0.4.3...v0.4.4) (2026-08-21)


### Bug Fixes

* **kryphos:** delete the dead vault types, and the wrong SALT_LEN they carried ([#445](https://github.com/forkwright/akroasis/issues/445)) ([e6ab46d](https://github.com/forkwright/akroasis/commit/e6ab46d3dded1bee118825af48f90f7d496bd807))

## [0.4.3](https://github.com/forkwright/akroasis/compare/v0.4.2...v0.4.3) (2026-08-21)


### Bug Fixes

* **koinon:** make the seal rename durable, and prove every stage recovers ([#443](https://github.com/forkwright/akroasis/issues/443)) ([08ff014](https://github.com/forkwright/akroasis/commit/08ff014a48afdcf638c6b6f6d9034ba50e1a4038))
* **kryphos:** close the wave-1 batch — nonce, audit ordering, lifecycle gate, UTF-8 coverage ([#441](https://github.com/forkwright/akroasis/issues/441)) ([1b972ee](https://github.com/forkwright/akroasis/commit/1b972ee1302636c5c0d517fc73619515ee2c1f36))

## [0.4.2](https://github.com/forkwright/akroasis/compare/v0.4.1...v0.4.2) (2026-08-21)


### Bug Fixes

* **kerykeion:** accept only the PSK shapes the protocol defines ([#436](https://github.com/forkwright/akroasis/issues/436)) ([c04d9a2](https://github.com/forkwright/akroasis/commit/c04d9a27854c4fb3c5ce713acc009d023a4bd8af))
* **kerykeion:** bound the outbound pending queue ([#438](https://github.com/forkwright/akroasis/issues/438)) ([35a5d74](https://github.com/forkwright/akroasis/commit/35a5d744a768dfab90ff94d2ec1e9d865b3c4a13))
* **kerykeion:** bound the wire fields that were trusted to bound themselves ([#435](https://github.com/forkwright/akroasis/issues/435)) ([e7b4f45](https://github.com/forkwright/akroasis/commit/e7b4f451a68159b5ce0dbc5741c15d89e8086c7e))
* **kerykeion:** bound what a peer can accumulate during the handshake ([#437](https://github.com/forkwright/akroasis/issues/437)) ([04c22d7](https://github.com/forkwright/akroasis/commit/04c22d74a0500c5e09c45543cbcd420edf24fdc3))
* **kryphos:** derive the vault key from the parameters the vault recorded ([#434](https://github.com/forkwright/akroasis/issues/434)) ([203d75f](https://github.com/forkwright/akroasis/commit/203d75f64f2a9f8b6f55dfa1f9e5f9db32fd7c5d)), closes [#231](https://github.com/forkwright/akroasis/issues/231)
* **kryphos:** reject a bad vault salt instead of panicking on it ([#432](https://github.com/forkwright/akroasis/issues/432)) ([486c78f](https://github.com/forkwright/akroasis/commit/486c78ffccf26a5d8b5044a3f46aaea5a89127ed))
* **kryphos:** report what actually failed, not the nearest familiar error ([#439](https://github.com/forkwright/akroasis/issues/439)) ([de44d50](https://github.com/forkwright/akroasis/commit/de44d50e662afdb8ee20fa5aa5b1110b8f72ade7))
* **semaino:** make the signal path lossless end to end ([#429](https://github.com/forkwright/akroasis/issues/429)) ([f6249d5](https://github.com/forkwright/akroasis/commit/f6249d5e91db0708a03cb49cc6e51925053588ba)), closes [#232](https://github.com/forkwright/akroasis/issues/232)
* **syntonia:** neutralize spreadsheet formulas in exported channel names ([#433](https://github.com/forkwright/akroasis/issues/433)) ([41a41d6](https://github.com/forkwright/akroasis/commit/41a41d6d2a2cd69e704d9a066ae175a3480c5003))

## [0.4.1](https://github.com/forkwright/akroasis/compare/v0.4.0...v0.4.1) (2026-08-21)


### Bug Fixes

* **semaino:** order ingestion before scoring on one path ([#426](https://github.com/forkwright/akroasis/issues/426)) ([2e0dd26](https://github.com/forkwright/akroasis/commit/2e0dd26dedf36fc91e82d23db5ec198b1a15eb67)), closes [#224](https://github.com/forkwright/akroasis/issues/224)

## [0.4.0](https://github.com/forkwright/akroasis/compare/v0.3.0...v0.4.0) (2026-08-21)


### Features

* **kerykeion:** add the BLE transport and its injectable scan/GATT seam ([#420](https://github.com/forkwright/akroasis/issues/420)) ([f9fc620](https://github.com/forkwright/akroasis/commit/f9fc6204c85b5645b6b753c8b49dde82fbdf7fd0))


### Bug Fixes

* **akroasis-server:** validate the serial port path before it reaches a device open ([#421](https://github.com/forkwright/akroasis/issues/421)) ([47eb3a9](https://github.com/forkwright/akroasis/commit/47eb3a93a95b4c62265eb6f5538c58d53078d12c))
* **akroasis:** zeroize vault passphrase and new-secret terminal input ([#425](https://github.com/forkwright/akroasis/issues/425)) ([97b7b2f](https://github.com/forkwright/akroasis/commit/97b7b2f08238f3b07d8e0fafc6f89ed65ca8b245)), closes [#379](https://github.com/forkwright/akroasis/issues/379)
* **semaino:** scope convergence detection to the signal's own cell ([#424](https://github.com/forkwright/akroasis/issues/424)) ([bd8e569](https://github.com/forkwright/akroasis/commit/bd8e569abb853f78ac8272e0eed6dd21e6ad7518))

## [0.3.0](https://github.com/forkwright/akroasis/compare/v0.2.0...v0.3.0) (2026-08-21)


### Features

* **security:** add validated caller contract ([#412](https://github.com/forkwright/akroasis/issues/412)) ([040eccb](https://github.com/forkwright/akroasis/commit/040eccb73ccbd6e133da3412f92f923a274ccc73))


### Bug Fixes

* **ci:** report every gate failure, not whichever one is reached first ([#415](https://github.com/forkwright/akroasis/issues/415)) ([1efdad9](https://github.com/forkwright/akroasis/commit/1efdad991bbcf4044092f6e44fb8be9937ccd04c)), closes [#414](https://github.com/forkwright/akroasis/issues/414)

## [0.2.0](https://github.com/forkwright/akroasis/compare/v0.1.25...v0.2.0) (2026-08-19)


### Features

* **kerykeion:** author the wire schema from the protocol, dropping the vendored GPL protos ([#392](https://github.com/forkwright/akroasis/issues/392)) ([aa81813](https://github.com/forkwright/akroasis/commit/aa8181353a3a8e28f147b308877be86fcd6eedb7))


### Bug Fixes

* **koinon:** close three tamper-log integrity gaps in the writer path ([#391](https://github.com/forkwright/akroasis/issues/391)) ([c947eee](https://github.com/forkwright/akroasis/commit/c947eee7e73237ff7fd31146ccd171e199436f04))

## [0.1.25](https://github.com/forkwright/akroasis/compare/v0.1.24...v0.1.25) (2026-08-17)


### Bug Fixes

* **ci:** stop main-push gate self-cancelling via caller-level concurrency ([#375](https://github.com/forkwright/akroasis/issues/375)) ([4daff33](https://github.com/forkwright/akroasis/commit/4daff3303e360676247015285d55472c050b7366))
* **docs:** remove the internal forge hostname and add the doc manifest ([fa91efa](https://github.com/forkwright/akroasis/commit/fa91efaf0187bf24eb1a9592a85654ed2f4a7cd4))
* **kerykeion:** attribute mesh source to the verified sender, not the packet's claim ([#381](https://github.com/forkwright/akroasis/issues/381)) ([a1a5a3b](https://github.com/forkwright/akroasis/commit/a1a5a3b5419f44ce758e4aa244485f41742e2d66))
* **kerykeion:** bound node and link cardinality, stop treating unknown routing codes as ACKs ([#384](https://github.com/forkwright/akroasis/issues/384)) ([a51f885](https://github.com/forkwright/akroasis/commit/a51f8858df354f2fd6f6d25bcb494ef633ffc737))
* **kryphos:** reject empty vault passphrases, bind ciphertext to entry identity, serialize mutations ([831ba23](https://github.com/forkwright/akroasis/commit/831ba23d980048a85d3b4d4d564d5f8460ddeac7)), closes [#287](https://github.com/forkwright/akroasis/issues/287) [#283](https://github.com/forkwright/akroasis/issues/283) [#214](https://github.com/forkwright/akroasis/issues/214)
* **kryphos:** zeroize decrypted secrets and encrypt credential metadata at rest ([#382](https://github.com/forkwright/akroasis/issues/382)) ([07b421c](https://github.com/forkwright/akroasis/commit/07b421c31b520e89f9db4ded3b19469981c5c3a7))

## [0.1.24](https://github.com/forkwright/akroasis/compare/v0.1.23...v0.1.24) (2026-08-14)


### Bug Fixes

* **ci:** make the dependabot auto-merge guard refuse instead of merging ([#372](https://github.com/forkwright/akroasis/issues/372)) ([56147fb](https://github.com/forkwright/akroasis/commit/56147fbb7995b29261f2b2603903c40ad7807b2a)), closes [#371](https://github.com/forkwright/akroasis/issues/371)

## [0.1.23](https://github.com/forkwright/akroasis/compare/v0.1.22...v0.1.23) (2026-08-09)


### Bug Fixes

* **akroasis-server:** bound request timeout, concurrency, and body size ([#362](https://github.com/forkwright/akroasis/issues/362)) ([d77876d](https://github.com/forkwright/akroasis/commit/d77876d1af58ebfdaa406bd4f3015cdda3d418da)), closes [#194](https://github.com/forkwright/akroasis/issues/194)
* **kerykeion:** escalate background task failures to a clean shutdown ([#363](https://github.com/forkwright/akroasis/issues/363)) ([0a468f1](https://github.com/forkwright/akroasis/commit/0a468f11996be9c4aa67be137762c3e4038c6211)), closes [#205](https://github.com/forkwright/akroasis/issues/205)
* **semaino:** bound per-cell convergence storage to one hit per domain ([#364](https://github.com/forkwright/akroasis/issues/364)) ([cba9170](https://github.com/forkwright/akroasis/commit/cba91700705b3ec73ee711709eb943eff26d9254)), closes [#223](https://github.com/forkwright/akroasis/issues/223)
* **semaino:** ingest a trigger signal before detecting its convergence ([#365](https://github.com/forkwright/akroasis/issues/365)) ([de0b8ae](https://github.com/forkwright/akroasis/commit/de0b8ae2be29a60921ec0ace9cf64ad51e022968)), closes [#224](https://github.com/forkwright/akroasis/issues/224)

## [0.1.22](https://github.com/forkwright/akroasis/compare/v0.1.21...v0.1.22) (2026-08-04)


### Bug Fixes

* **kerykeion:** seed PacketProcessor's NodeDb and route runtime NodeInfo to it ([#360](https://github.com/forkwright/akroasis/issues/360)) ([9e8778b](https://github.com/forkwright/akroasis/commit/9e8778b43263621cf55b48c449b62421f3925781))

## [0.1.21](https://github.com/forkwright/akroasis/compare/v0.1.20...v0.1.21) (2026-08-04)


### Bug Fixes

* **lint:** burn down the kanon-lint baseline — 104 entries down to 25 ([#358](https://github.com/forkwright/akroasis/issues/358)) ([7004d73](https://github.com/forkwright/akroasis/commit/7004d734aba92ce404f003f2deca6885b9525f84))

## [0.1.20](https://github.com/forkwright/akroasis/compare/v0.1.19...v0.1.20) (2026-08-04)


### Bug Fixes

* **api:** split ApiError into client message and server detail ([#354](https://github.com/forkwright/akroasis/issues/354)) ([026b5d0](https://github.com/forkwright/akroasis/commit/026b5d018de0dabd71c1e1942cba41c4efbd1c10)), closes [#227](https://github.com/forkwright/akroasis/issues/227)
* **config:** deny unknown fields on operator-authored config types ([#344](https://github.com/forkwright/akroasis/issues/344)) ([2026402](https://github.com/forkwright/akroasis/commit/202640250e33cb50371ce8d1f831916013bba08f)), closes [#261](https://github.com/forkwright/akroasis/issues/261)
* **gate:** compile and test hardware-serial feature in gate stages ([#355](https://github.com/forkwright/akroasis/issues/355)) ([bfef8be](https://github.com/forkwright/akroasis/commit/bfef8be71a3ebfa0c6b7d74df689686819a1cdfa))
* **kerykeion:** close six correctness and resilience findings from the wave-1 audit ([#348](https://github.com/forkwright/akroasis/issues/348)) ([6d75b31](https://github.com/forkwright/akroasis/commit/6d75b3190d6fa5c1fee8b603acff31cc552b828e)), closes [#229](https://github.com/forkwright/akroasis/issues/229)
* **kerykeion:** reject unknown keys in mesh config deserialization ([#343](https://github.com/forkwright/akroasis/issues/343)) ([cdf664e](https://github.com/forkwright/akroasis/commit/cdf664ead7b084ded0b4daadc3aa1b9fa557160e)), closes [#261](https://github.com/forkwright/akroasis/issues/261)
* **koinon:** validate deserialized values and stop the writer emitting unverifiable log entries ([#350](https://github.com/forkwright/akroasis/issues/350)) ([20e7206](https://github.com/forkwright/akroasis/commit/20e7206ca84ca907be0fd1ba46cfb6d1c123d441)), closes [#230](https://github.com/forkwright/akroasis/issues/230)
* **radio:** honor CSV quoting on import and preserve probe failure causes ([#347](https://github.com/forkwright/akroasis/issues/347)) ([81d45e4](https://github.com/forkwright/akroasis/commit/81d45e40d3667ff26a61d5687a8d6cf5c1dbc82d)), closes [#228](https://github.com/forkwright/akroasis/issues/228)
* **semaino:** reclaim expired alert-suppression entries ([#346](https://github.com/forkwright/akroasis/issues/346)) ([77f4427](https://github.com/forkwright/akroasis/commit/77f4427739013e8a97787ea82017d18a3495f54c)), closes [#222](https://github.com/forkwright/akroasis/issues/222)
* **semaino:** report the baseline the score was computed against ([#351](https://github.com/forkwright/akroasis/issues/351)) ([0041e49](https://github.com/forkwright/akroasis/commit/0041e492e668e87f52953045e0f0e4d63606ad85)), closes [#232](https://github.com/forkwright/akroasis/issues/232)
* **syntonia:** close nine wave-1 audit findings and consolidate detection tables ([#342](https://github.com/forkwright/akroasis/issues/342)) ([78c5a0c](https://github.com/forkwright/akroasis/commit/78c5a0c45bb067e750bfe6eef91b578d303d6d5d)), closes [#233](https://github.com/forkwright/akroasis/issues/233)
* **vault:** avoid mutating vault dir on failed open of uninitialized path ([#356](https://github.com/forkwright/akroasis/issues/356)) ([44b1f57](https://github.com/forkwright/akroasis/commit/44b1f57515cd1dddd08227fcb839e98def156084)), closes [#286](https://github.com/forkwright/akroasis/issues/286)

## [0.1.19](https://github.com/forkwright/akroasis/compare/v0.1.18...v0.1.19) (2026-07-31)


### Bug Fixes

* **syntonia:** add assertion messages and clear the dead bare-assert baseline family ([#339](https://github.com/forkwright/akroasis/issues/339)) ([2e15da5](https://github.com/forkwright/akroasis/commit/2e15da52e250eb1f5c98bc4cd8065a597bb78172)), closes [#261](https://github.com/forkwright/akroasis/issues/261)

## [0.1.18](https://github.com/forkwright/akroasis/compare/v0.1.17...v0.1.18) (2026-07-29)


### Bug Fixes

* **syntonia:** make EEPROM download/upload respect the variant's aux block ([#334](https://github.com/forkwright/akroasis/issues/334)) ([b63f0cb](https://github.com/forkwright/akroasis/commit/b63f0cb523dc33a897928d051944ce8eb0688645)), closes [#225](https://github.com/forkwright/akroasis/issues/225)

## [0.1.17](https://github.com/forkwright/akroasis/compare/v0.1.16...v0.1.17) (2026-07-29)


### Bug Fixes

* **kerykeion:** drive retention and TTL maintenance from the router flush tick ([#332](https://github.com/forkwright/akroasis/issues/332)) ([dc70325](https://github.com/forkwright/akroasis/commit/dc70325f7d289736f3ab1759025886e726f5c215)), closes [#244](https://github.com/forkwright/akroasis/issues/244)

## [0.1.16](https://github.com/forkwright/akroasis/compare/v0.1.15...v0.1.16) (2026-07-29)


### Bug Fixes

* **koinon,kerykeion:** guard non-finite baseline observations and cap over-capacity snapshots ([#330](https://github.com/forkwright/akroasis/issues/330)) ([4565908](https://github.com/forkwright/akroasis/commit/45659084c0b8554e78f1bba0bc96b3ec9882bc6c))

## [0.1.15](https://github.com/forkwright/akroasis/compare/v0.1.14...v0.1.15) (2026-07-29)


### Bug Fixes

* **release:** keep Cargo.lock in lockstep with the workspace version ([#328](https://github.com/forkwright/akroasis/issues/328)) ([f8d7175](https://github.com/forkwright/akroasis/commit/f8d7175d96345cd824582c7006c6056fb938b3d4)), closes [#327](https://github.com/forkwright/akroasis/issues/327)

## [0.1.14](https://github.com/forkwright/akroasis/compare/v0.1.13...v0.1.14) (2026-07-28)


### Bug Fixes

* **akroasis:** reject overflowing chirp csv frequency offsets ([#197](https://github.com/forkwright/akroasis/issues/197)) ([#309](https://github.com/forkwright/akroasis/issues/309)) ([0ef72bb](https://github.com/forkwright/akroasis/commit/0ef72bb343f9584e9f2a6b6ac4e62ac4e25faba4))
* **akroasis:** retrieve vault secrets byte-exact instead of lossy utf8 ([#196](https://github.com/forkwright/akroasis/issues/196)) ([#308](https://github.com/forkwright/akroasis/issues/308)) ([80f952c](https://github.com/forkwright/akroasis/commit/80f952caaaef46cead3cd5f8f60a6bdf881abf2e))
* **kerykeion:** bound store-forward destination-map cardinality ([#242](https://github.com/forkwright/akroasis/issues/242)) ([#311](https://github.com/forkwright/akroasis/issues/311)) ([94f5dfa](https://github.com/forkwright/akroasis/commit/94f5dfa19837d946931d6cae52e3f2aac770d75f))
* **kerykeion:** clamp config-sourced hop_limit to MAX_HOP_LIMIT ([#240](https://github.com/forkwright/akroasis/issues/240)) ([#305](https://github.com/forkwright/akroasis/issues/305)) ([43025c8](https://github.com/forkwright/akroasis/commit/43025c8a51d24a080180d226a59a0366a5f1d17b))
* **kerykeion:** correct direct-link hop_count predicate to 0 ([#201](https://github.com/forkwright/akroasis/issues/201)) ([#303](https://github.com/forkwright/akroasis/issues/303)) ([138bbec](https://github.com/forkwright/akroasis/commit/138bbecdc5147c4ce084ab8d195f5c94b392ea94))
* **kerykeion:** evict stale nodes from topology, not just stale links ([#299](https://github.com/forkwright/akroasis/issues/299)) ([b8aa81d](https://github.com/forkwright/akroasis/commit/b8aa81d26533f3f1ecb3598d325ba51ef4ec2def)), closes [#243](https://github.com/forkwright/akroasis/issues/243)
* **kerykeion:** guard mark_acknowledged against duplicate/late ACKs ([#199](https://github.com/forkwright/akroasis/issues/199)) ([#291](https://github.com/forkwright/akroasis/issues/291)) ([7879844](https://github.com/forkwright/akroasis/commit/787984410698258e606e5a9d6a27448e857c9f54))
* **kerykeion:** guard topology staleness cutoff against clock underflow ([#293](https://github.com/forkwright/akroasis/issues/293)) ([18e59c1](https://github.com/forkwright/akroasis/commit/18e59c1ad050db08ccd98769a4293830b2397093)), closes [#206](https://github.com/forkwright/akroasis/issues/206)
* **kerykeion:** make is_partitioned agree with connected_components ([#202](https://github.com/forkwright/akroasis/issues/202)) ([#313](https://github.com/forkwright/akroasis/issues/313)) ([21863c8](https://github.com/forkwright/akroasis/commit/21863c86f97405cb3a93444c7e216cb011eb2d95))
* **kerykeion:** preserve message TTL/created across outbound retry ([#200](https://github.com/forkwright/akroasis/issues/200)) ([#292](https://github.com/forkwright/akroasis/issues/292)) ([53491d6](https://github.com/forkwright/akroasis/commit/53491d6712a42eaa8da7bfe18dea852b82a08359))
* **kerykeion:** reject non-finite SNR in topology update_link ([#203](https://github.com/forkwright/akroasis/issues/203)) ([#304](https://github.com/forkwright/akroasis/issues/304)) ([8de2e5d](https://github.com/forkwright/akroasis/commit/8de2e5dc69d62dad3364f35804ab548ba2c9401e))
* **kerykeion:** reject non-finite snr_ceiling at config deserialization ([#241](https://github.com/forkwright/akroasis/issues/241)) ([#306](https://github.com/forkwright/akroasis/issues/306)) ([1d4c9f6](https://github.com/forkwright/akroasis/commit/1d4c9f607d3279ceac2103251c61d3b35f21eb2b))
* **kerykeion:** stop starving heartbeat/router-flush behind main recv() lock ([#190](https://github.com/forkwright/akroasis/issues/190)) ([#289](https://github.com/forkwright/akroasis/issues/289)) ([cb57cb8](https://github.com/forkwright/akroasis/commit/cb57cb8751e80b0921348b6417f1da2734c2cd63))
* **kerykeion:** track delivery only after dispatch succeeds ([#245](https://github.com/forkwright/akroasis/issues/245)) ([#300](https://github.com/forkwright/akroasis/issues/300)) ([3f6c484](https://github.com/forkwright/akroasis/commit/3f6c484c992d248e20c99fcf8129d9dbd0866761))
* **kerykeion:** treat hops_away 0 as direct, matching the sibling path ([#325](https://github.com/forkwright/akroasis/issues/325)) ([1c160b5](https://github.com/forkwright/akroasis/commit/1c160b50b6d02b2e5ebb013a683034363fff49f8)), closes [#321](https://github.com/forkwright/akroasis/issues/321)
* **koinon:** propagate non-eof io errors from verify_chain reads ([#210](https://github.com/forkwright/akroasis/issues/210)) ([#310](https://github.com/forkwright/akroasis/issues/310)) ([3e093d5](https://github.com/forkwright/akroasis/commit/3e093d518c678f7f17ccfdb86633f2e0d933f4d6))
* **koinon:** score zero-variance baseline deviations as anomalous ([#212](https://github.com/forkwright/akroasis/issues/212)) ([#307](https://github.com/forkwright/akroasis/issues/307)) ([8b44e4f](https://github.com/forkwright/akroasis/commit/8b44e4fbae0af56e7c801720e9cc4fc01c2a4c23))
* **kryphos:** report unseal key-parse failure as KeyParse not EncryptionFailed ([#216](https://github.com/forkwright/akroasis/issues/216)) ([#314](https://github.com/forkwright/akroasis/issues/314)) ([a2bc7d5](https://github.com/forkwright/akroasis/commit/a2bc7d5315ccacf3393cdcf8a930da616ea90da9))
* **semaino:** floor cell quantization for negative coordinates ([#221](https://github.com/forkwright/akroasis/issues/221)) ([#295](https://github.com/forkwright/akroasis/issues/295)) ([9e26eea](https://github.com/forkwright/akroasis/commit/9e26eea431496c7570df14142e89584db504fe9f))
* **semaino:** quantize alert cells at configured grid_resolution ([#294](https://github.com/forkwright/akroasis/issues/294)) ([48e4ca4](https://github.com/forkwright/akroasis/commit/48e4ca4262689bac620ad690cb4b5e3c395da0d7)), closes [#220](https://github.com/forkwright/akroasis/issues/220)
* **syntonia:** remove duplicate baofeng magic-byte constants ([#237](https://github.com/forkwright/akroasis/issues/237)) ([#296](https://github.com/forkwright/akroasis/issues/296)) ([456ec9d](https://github.com/forkwright/akroasis/commit/456ec9d4967e47caafd5d69c2ed2bb6ecb922aa6))
* **syntonia:** repair protocol_tests.rs API drift ([#239](https://github.com/forkwright/akroasis/issues/239)) ([#298](https://github.com/forkwright/akroasis/issues/298)) ([d47387c](https://github.com/forkwright/akroasis/commit/d47387c9943d98664164a7bf6c664780368b8a5f))
* **syntonia:** skip out-of-range channel index instead of truncating to 0 ([#290](https://github.com/forkwright/akroasis/issues/290)) ([5fd1156](https://github.com/forkwright/akroasis/commit/5fd1156c1ee15a94b4f258c267eb56cc6e8e22c0)), closes [#193](https://github.com/forkwright/akroasis/issues/193)
* **syntonia:** wire baofeng::detect into the crate module tree ([#238](https://github.com/forkwright/akroasis/issues/238)) ([#297](https://github.com/forkwright/akroasis/issues/297)) ([2aafbe9](https://github.com/forkwright/akroasis/commit/2aafbe95a1d163bde7c52d6a196cb52fd496f24c))

## [0.1.13](https://github.com/forkwright/akroasis/compare/v0.1.12...v0.1.13) (2026-07-23)


### Features

* **akroasis-server:** scaffold axum HTTP backend; lock desktop-first surface ([#118](https://github.com/forkwright/akroasis/issues/118), [#126](https://github.com/forkwright/akroasis/issues/126)) ([#174](https://github.com/forkwright/akroasis/issues/174)) ([2c06653](https://github.com/forkwright/akroasis/commit/2c06653dc51facb47804f7acc2aca2728b9cf28c))
* **akroasis:** add direct-port radio detection ([#171](https://github.com/forkwright/akroasis/issues/171)) ([350c268](https://github.com/forkwright/akroasis/commit/350c2687c316f5348822674f53648d518ef4a4f3))
* **akroasis:** add mesh json reports ([#161](https://github.com/forkwright/akroasis/issues/161)) ([fb99ce9](https://github.com/forkwright/akroasis/commit/fb99ce9e0a51e5a31b87a2a873ba6b6a669e2e30))
* **akroasis:** add radio detect json report ([#160](https://github.com/forkwright/akroasis/issues/160)) ([a3bf2db](https://github.com/forkwright/akroasis/commit/a3bf2db4c66ff33d5088efd704c5138cf707354a))
* **akroasis:** add radio export json report ([#162](https://github.com/forkwright/akroasis/issues/162)) ([e8b3e91](https://github.com/forkwright/akroasis/commit/e8b3e91837198d6fbe645300b15c54976cd27b0d))
* **akroasis:** add radio import json report ([6933828](https://github.com/forkwright/akroasis/commit/6933828c9da90f2d2babb78c896129d3d27004db))
* **akroasis:** add vault identity json report ([98fa796](https://github.com/forkwright/akroasis/commit/98fa7965f6608fba3c8fa193ac5fbfc05507def5))
* **akroasis:** add vault list json report ([#170](https://github.com/forkwright/akroasis/issues/170)) ([45b406e](https://github.com/forkwright/akroasis/commit/45b406e64ff65cd5ec73bfe89c13c4bc7a5b1499))
* **akroasis:** wire opt-in radio detect hardware ([#159](https://github.com/forkwright/akroasis/issues/159)) ([46032ae](https://github.com/forkwright/akroasis/commit/46032ae4f7fef3c88ede8e43c42931a05de54d4d)), closes [#122](https://github.com/forkwright/akroasis/issues/122)
* **kerykeion:** instrument runtime async entrypoints ([#150](https://github.com/forkwright/akroasis/issues/150)) ([a9693c8](https://github.com/forkwright/akroasis/commit/a9693c88b623f45dc2c9e886ecc8c1f33eafd31a)), closes [#124](https://github.com/forkwright/akroasis/issues/124)
* **kerykeion:** live-reload bridge health interval ([ea632c0](https://github.com/forkwright/akroasis/commit/ea632c0e424034a34191447e6b02db20b172787e)), closes [#142](https://github.com/forkwright/akroasis/issues/142)
* **kryphos:** audit vault mutations ([0e30ee9](https://github.com/forkwright/akroasis/commit/0e30ee97f27073d2cf121fd210fcf6f736ec2128))
* **semaino:** instrument runtime async entrypoints ([#156](https://github.com/forkwright/akroasis/issues/156)) ([0509df7](https://github.com/forkwright/akroasis/commit/0509df73153969d5fdd59edaf1f6975bff67bbbf)), closes [#124](https://github.com/forkwright/akroasis/issues/124)
* **sphragis:** repoint to standalone forkwright/sphragis git dep ([#176](https://github.com/forkwright/akroasis/issues/176)) ([eff13bd](https://github.com/forkwright/akroasis/commit/eff13bd68ac786f2bd4d18bff6533df91595f713))
* **syntonia,akroasis:** public protocol API + BaofengProtocolSession ([#122](https://github.com/forkwright/akroasis/issues/122)) ([#175](https://github.com/forkwright/akroasis/issues/175)) ([80cd09a](https://github.com/forkwright/akroasis/commit/80cd09a91671262f5642a21bd6ffbf8df7606d7d))


### Bug Fixes

* **akroasis:** remove unwired daemon and mesh send surfaces ([#148](https://github.com/forkwright/akroasis/issues/148)) ([ec68ab9](https://github.com/forkwright/akroasis/commit/ec68ab9e926b9f85afe77c67a6305158d56087d6))
* **deps:** clear RUSTSEC-2026-0190/-0204 + yanked crates via lockfile bumps ([#260](https://github.com/forkwright/akroasis/issues/260)) ([80c8516](https://github.com/forkwright/akroasis/commit/80c85164b6deaa14f66c02e0a69fc0957993d6b4))
* **kerykeion:** recover receive stream from malformed protobuf payloads ([#275](https://github.com/forkwright/akroasis/issues/275)) ([746e8e3](https://github.com/forkwright/akroasis/commit/746e8e35a5460e48de585416e2a4cab24678d866))
* **kerykeion:** store-and-forward preserves payload + real stored_at_ms ([#272](https://github.com/forkwright/akroasis/issues/272)) ([f93ef9a](https://github.com/forkwright/akroasis/commit/f93ef9ada4f02ec508b311191f0e6f1e2190b4f7)), closes [#189](https://github.com/forkwright/akroasis/issues/189) [#236](https://github.com/forkwright/akroasis/issues/236)
* **kerykeion:** wire RoutingProcessor ACK/NAK into the collector receive loop ([#274](https://github.com/forkwright/akroasis/issues/274)) ([14498ae](https://github.com/forkwright/akroasis/commit/14498ae99b74a8282e7af1b9bffd039d328ad89b))
* **koinon:** key the tamper-log hash chain and seal trailing truncation ([#276](https://github.com/forkwright/akroasis/issues/276)) ([08ed40f](https://github.com/forkwright/akroasis/commit/08ed40f3577cdf663e4874eff3fe4f28ea76659e)), closes [#213](https://github.com/forkwright/akroasis/issues/213)
* **kryphos:** restrict vault dir and files to owner-only perms on Unix ([e50f966](https://github.com/forkwright/akroasis/commit/e50f9662b5e97424097543af5ca6866708599ed5)), closes [#217](https://github.com/forkwright/akroasis/issues/217)
* **lint:** satisfy unwired-dead-code-untracked markers + clear pre-existing clippy/fmt fails ([#267](https://github.com/forkwright/akroasis/issues/267)) ([3987d54](https://github.com/forkwright/akroasis/commit/3987d54df851e9184a39394e162e640d9a3b7ad3))
* **syntonia:** unify baofeng RadioIdent + non-exhaustive RadioVariant arm ([#268](https://github.com/forkwright/akroasis/issues/268)) ([76583b8](https://github.com/forkwright/akroasis/commit/76583b871b28bfd6d9b8005ccb5869203f053a9c)), closes [#265](https://github.com/forkwright/akroasis/issues/265)

## [0.1.12](https://github.com/forkwright/akroasis/compare/v0.1.11...v0.1.12) (2026-05-21)


### Features

* **_llm:** add T0 corpus per [#667](https://github.com/forkwright/akroasis/issues/667) / [#673](https://github.com/forkwright/akroasis/issues/673) fleet rollout ([#12](https://github.com/forkwright/akroasis/issues/12)) ([973b096](https://github.com/forkwright/akroasis/commit/973b09663a789ecc611139210ad48db0317f16d4))


### Bug Fixes

* **akroasis:** allow too_many_lines on parse_chirp_csv with WHY ([815f8b2](https://github.com/forkwright/akroasis/commit/815f8b21779c1951dbb3cf00e8d622d95617c3f0))
* **ci:** raise cargo nextest stage timeout for cold-compile path ([#111](https://github.com/forkwright/akroasis/issues/111)) ([b6dde93](https://github.com/forkwright/akroasis/commit/b6dde93ca9ada761c0f09a4d5f450b8982c622ab))
* **kerykeion:** remove stale expect_used expectations and .get(0) anti-patterns ([c83f80d](https://github.com/forkwright/akroasis/commit/c83f80d319240c03e71aef90c6a13c8bdf038d52))
* **koinon:** non-snake-case, hex grouping, indexing panics in tests ([6353c81](https://github.com/forkwright/akroasis/commit/6353c815be48817628774b0a83debdbcee94ecd8))
* **syntonia:** restore hardware-serial feature and fix stub lint debt ([5926e3e](https://github.com/forkwright/akroasis/commit/5926e3e96b8c6a027e2cfc1e930d6a9b3d4788f3))

## [0.1.11](https://github.com/forkwright/akroasis/compare/v0.1.10...v0.1.11) (2026-04-15)


### Bug Fixes

* **ci:** fix gate-attestation job name and fetch base branch ([#100](https://github.com/forkwright/akroasis/issues/100)) ([d228f26](https://github.com/forkwright/akroasis/commit/d228f26e99050909cb2f68f8379fc72ad8e0816e))

## [0.1.10](https://github.com/forkwright/akroasis/compare/v0.1.9...v0.1.10) (2026-04-13)


### Features

* **semaino:** signal aggregation, convergence detection, alert pipeline ([99335ab](https://github.com/forkwright/akroasis/commit/99335abf7767652f1319b795450b14555bbc0ec0)), closes [#85](https://github.com/forkwright/akroasis/issues/85)
* **syntonia:** scaffold Yaesu FTM-510DR module ([e2e87fe](https://github.com/forkwright/akroasis/commit/e2e87fed75c56712dbe8cb7694064236cbda4c10)), closes [#80](https://github.com/forkwright/akroasis/issues/80)


### Bug Fixes

* **ops:** full AGPL-3.0 text, AI training prohibition, disclaimer ([76e2e49](https://github.com/forkwright/akroasis/commit/76e2e4981d0d2d21b517164928fe78ec62bc2364)), closes [#75](https://github.com/forkwright/akroasis/issues/75)
* **syntonia:** port hardware/usb.rs from nusb to rusb ([e511170](https://github.com/forkwright/akroasis/commit/e511170764b8ae9379aaa462caa383d65ae83a92)), closes [#82](https://github.com/forkwright/akroasis/issues/82)
* undo mangled identifiers from botched kanon lint --fix run ([ea46319](https://github.com/forkwright/akroasis/commit/ea4631979b299e2aebc09b6f03d0fb6f26c2c4c2))

## [0.1.9](https://github.com/forkwright/akroasis/compare/v0.1.8...v0.1.9) (2026-04-04)


### Bug Fixes

* resolve lint violations via kanon lint --fix ([e96296b](https://github.com/forkwright/akroasis/commit/e96296b4808d41f0bee2d94d983dc00b89225ead))

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
