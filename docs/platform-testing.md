# Platform testing

CI runs the full automated suite on current macOS and Linux runners.

For WSL acceptance, clone the repository beneath the distribution's Linux home
filesystem (for example `/home/<user>/skillator`) and run:

```console
cargo test --all-targets
```

Repeat the mounted-filesystem capability cases with temporary Targets under
`/mnt/c`. Unsupported link or rename capabilities must be reported without
changing the requested Materialization kind or replacing existing content.
Native Windows is outside the MVP support boundary.
