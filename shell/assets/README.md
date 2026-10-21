# Shell assets

## fonts/

`Inter-SemiBold.ttf` is Inter 4.1 by Rasmus Andersson and the Inter Project
Authors, unmodified, under the SIL Open Font License 1.1 (`Inter-LICENSE.txt`,
which names no Reserved Font Name). `shell/.../text.rs` builds it into the
binary, so the IPK carries `Inter-LICENSE.txt` beside it (the Makefile's
`ipk` rule).

It comes from `extras/ttf/` in the release zip,
<https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip>, fetched
on 2026-10-20. GitHub publishes no digest for that asset, so these are the
hashes seen then:

| File | SHA-256 |
|---|---|
| `Inter-4.1.zip` | `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e` |
| `Inter-SemiBold.ttf` | `78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3` |
| `Inter-LICENSE.txt` (the zip's `LICENSE.txt`) | `262481e844521b326f5ecd053e59b98c8b2da78c8ee1bdbb6e8174305e54935a` |
