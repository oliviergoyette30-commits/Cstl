---
name: Reproduction report
about: You ran ./reproduce.sh and want to report whether the claims held
title: "[REPRO] "
labels: reproduction
---

## Outcome

- [ ] All stages passed
- [ ] Some stages failed (details below)

## reproduce.sh output

## Numbers observed

| Claim (from README) | Reported | Observed on my machine |
|---------------------|----------|------------------------|
| Rust tests          | 41       |                        |
| Python tests        | 201      |                        |
| Compression vs JSON | ~44–52%  |                        |

## Environment

- OS:
- cargo --version:
- python3 --version:

## Notes

Anything that differed from the documented procedure, or context that might explain a discrepancy.
