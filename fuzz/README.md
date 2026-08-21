# Fuzz targets

Three libFuzzer targets over the surfaces that parse untrusted radio input, plus the seed corpus they
start from. Written for [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz).

## Running them

Nightly only — libFuzzer and its sanitizers are not on stable. The repository toolchain stays stable;
this crate is deliberately outside the workspace so that stays true.

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run frame_decode              # until you stop it
cargo +nightly fuzz run frame_decode -- -max_total_time=60   # bounded, as CI runs it
cargo +nightly fuzz list
```

A crash writes its reproducer to `fuzz/artifacts/<target>/`. Re-run one with:

```sh
cargo +nightly fuzz run frame_decode fuzz/artifacts/frame_decode/crash-<hash>
```

## The targets

| Target | Surface | Property |
|---|---|---|
| `frame_decode` | `codec::MeshCodec` | never panics; a yielded frame consumed input |
| `message_parse` | `FromRadio` / `ToRadio` | decoding fails cleanly; an accepted message re-encodes losslessly |
| `routing_decision` | `RoutingProcessor::process_routing` | every decodable packet gets a verdict |

`frame_decode` asserts progress rather than only absence of panic: a decoder that reports a frame
without consuming bytes turns `Framed`'s loop into a hang, which a panic-only check would not see.

`message_parse` asserts a round trip, so a parser that accepts more than the type can represent fails
here rather than silently producing a value the encoder cannot express.

## The corpus

`corpus/<target>/` holds hand-built seeds rather than captured traffic, each one a shape named for
what it exercises: valid frames, a frame split across the magic pair, a length field exceeding
`MAX_PACKET_SIZE`, a truncated varint, an unknown field number. The bytes are derived from the frame
layout documented at the top of `crates/kerykeion/src/codec.rs` and from protobuf wire encoding.

libFuzzer grows this corpus as it finds new coverage. Committing an input it discovers — particularly
one that reproduced a defect — is how a fixed bug stays fixed.

## CI

`.github/workflows/fuzz.yml` runs each target for 60 seconds on changes to this crate or to
`crates/kerykeion`, weekly, and on demand. That is a smoke test: it proves the targets build, load
their corpus and execute. Finding new defects is the scheduled run's job, and a longer local run's.
