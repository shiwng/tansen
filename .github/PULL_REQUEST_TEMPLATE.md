## Summary

<!-- Briefly describe what this pull request changes and why. -->

## Related Issues

<!-- Link related issues, e.g. Closes #123 -->

## Type of Change

- [ ] Bug fix
- [ ] New feature / enhancement
- [ ] Security hardening
- [ ] Documentation update
- [ ] CI / build / tooling

## Verification

Describe how you tested these changes:

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets`
- [ ] `cargo test --workspace`
- [ ] `pnpm --dir apps/desktop test`
- [ ] `pnpm --dir apps/desktop build`
- [ ] Manual smoke test in the desktop app (describe platform and steps)

## Security & Architecture Checklist

- [ ] No secrets, `.env` files, or credentials added
- [ ] Outbound URLs / redirects validated via `netguard` and response bodies bounded
- [ ] No blocking I/O, locks, or allocations added to the audio callback
- [ ] Documentation updated if behavior or settings changed
