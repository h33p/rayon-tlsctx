# rayon-tlsctx

[![Crates.io](https://img.shields.io/crates/v/rayon-tlsctx.svg)](https://crates.io/crates/rayon-tlsctx)
[![API Docs](https://docs.rs/rayon-tlsctx/badge.svg)](https://docs.rs/rayon-tlsctx)
[![Build and test](https://github.com/h33p/rayon-tlsctx/actions/workflows/build.yml/badge.svg)](https://github.com/h33p/rayon-tlsctx/actions/workflows/build.yml)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Thread local variables for Rayon thread pools

This crate provides a simple `ThreadLocalCtx` struct that allows to store efficient thread-local state that gets built by a lambda.

It is incredibly useful in multithreaded processing, where a context needs to be used that is expensive to clone. In the end, there will be no more clones occuring than number of threads in a rayon thread pool.
