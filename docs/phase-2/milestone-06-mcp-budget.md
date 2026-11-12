# P2-M6 MCP context baseline

Status: Reproducible **proxy estimate**, not Claude Code compatibility or
model-visible token proof. Run `python3 scripts/measure_mcp_corpus.py` from
the repository root. The source is the sanitized
[`mcp-corpus.json`](mcp-corpus.json); it contains no real TV address, token,
or certificate. The script materializes JSON Schemas from the proposed tool
fields and estimates `ceil(compact UTF-8 bytes / 4)`. It counts the server
instruction, advertised definitions, arguments, receipts, follow-up results,
and a duplicate result copy for `structuredContent` plus text compatibility.
Before counting, it verifies required mutation preconditions, Pairing and
forget guards, a full fingerprint, unique tool names and one sample result
per workflow call.
It excludes HTTP framing, cached prompt behavior, client-added prose, and
actual model tokenization. An isolated 2.1.300 CLI health check negotiated
`2026-07-28` with a fake endpoint; M10 must measure real model-visible
behavior in the installed target and verify the signed app endpoint.

| Corpus item | Calls | Tool search on proxy | All tools upfront proxy |
| --- | ---: | ---: | ---: |
| Inspect selected | 2 | 446 | 2,383 |
| Send remote action | 3 | 883 | 2,567 |
| Wake and inspect outcome | 3 | 834 | 2,568 |
| Discover, probe, Pair | 5 | 1,614 | 3,036 |
| Forget and inspect outcome | 4 | 1,121 | 2,763 |
| Stale selection | 2 | 629 | 2,452 |
| Lost or uncertain result | 3 | 891 | 2,575 |
| Fresh empty profile | 1 | 229 | 2,231 |
| Guarded Forget recovery | 3 | 889 | 2,610 |

Regenerated on 2026-11-12 after M8 froze canonical scoped IDs with full epoch
on every result, lossless counter strings, exact fingerprint/host confirmations,
selected-generation cancellation and separate guarded recovery commands. These
are proposed MCP summaries; no `/mcp` implementation is claimed.
`control_documentation` parses all proposed mutation samples with the real M8
HTTP parser and checks examples/results against the native codecs.
The 15-tool catalog is 2,053 proxy units plus 83 for the instruction; a
key-per-tool catalog has 28 tools and 3,636 units. The largest duplicate
result copy costs 339 units.

The catalog-plus-instruction proxy (2,136) exceeds the 1,900-token M10 target,
and the Pair workflow upfront proxy (3,036) exceeds 2,800. M10 must calibrate
actual client/model counts and optimize presentation without dropping guards;
these proxy overruns are recorded, not accepted as meeting the token gates.

M10 measurement gates, subject to real tokenizer/client calibration:

- Catalog plus instruction: at most 1,900 model-visible tokens for all tools
  upfront. The full-schema key-per-tool baseline is a comparison, not a
  permitted target for the compact catalog.
- Ordinary mutation receipt: at most 180 model-visible tokens including any
  text compatibility copy. Read summary: at most 300. A full certificate
  fingerprint or partial-cleanup result may use up to 450 rather than lose
  safety facts.
- Complete sanitized workflows: at most 1,700 with tool search on and 2,800
  with all tools upfront, including both copies of tool results. The budget
  includes the actual definitions exposed by the target client, not only
  server JSON bytes.
- Use the corpus call count when each short discovery/probe finishes within
  its bounded tool wait. Count one extra lookup for each slow effect rather
  than hiding it. Record model round trips separately: one decision turn per call
  plus any human confirmation turn, server reconnect, or extra status call.
  An output that saves tokens but induces more calls fails review.

The next measurement must capture Claude Code's actual tool-list view with
tool search enabled and disabled, emitted arguments, both result channels,
and model turns for each scenario. It must test a server initially absent,
sticky Stop, auth-header refresh after a rejection, and all-tools-upfront
fallback. No tool definition may disappear when the TV is disconnected; the
catalog changes only on server version or authorization change. No budget
permits dropping target IDs, epoch/generation, operation keys, complete
fingerprints, confirmation, or honest `uncertain`/unknown results.
