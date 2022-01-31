#!/bin/bash

(
  export RUST_BACKTRACE=1;
  cargo test --features test --all &&
  cargo check --benches --features test &&
  cargo check --all --no-default-features &&
  ([ "$TRAVIS_RUST_VERSION" != "nightly" ] || cargo test --features "test nightly" -p gluon compile_test)
)
