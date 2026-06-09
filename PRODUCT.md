# Merill Product Context

## Register

Product

## Purpose

Merill helps people in Malta understand one news event across multiple publishers. It clusters related reporting, identifies publisher ownership, and makes differences in coverage visible without claiming that ownership alone determines truth.

## Audience

- People in Malta who read news in English, Maltese, or both.
- Readers who want to compare reporting without opening many publisher sites.
- Readers who want to notice stories covered only by state-owned or party-owned media.

## Product Principles

1. Show evidence before interpretation. Headlines, publishers, timestamps, and ownership labels must remain inspectable.
2. Describe publisher ownership as context, not a verdict on article accuracy.
3. Explain every blindspot. A blindspot means no independent publisher was found in the current story group.
4. Keep local data private. Clustering, saved stories, and reading preferences remain on-device.
5. Preserve user intent. Saved stories survive reclustering and normal feed retention.
6. Keep React/Tauri and native Swift behavior aligned.

## Terminology

- **Story group:** Articles Merill believes describe the same event.
- **Perspective:** One publisher's report within a story group.
- **Ownership category:** State-owned, party-owned, church-owned, commercial independent, or investigative independent.
- **Blindspot:** A story group with coverage but no independent publisher represented.
- **Balanced:** Deterministic ordering based on freshness, publisher coverage, and independent representation.

## Trust Boundaries

- Merill does not rate factual truth.
- Publisher ownership metadata may be corrected by the user.
- Automated grouping can be wrong; users can remove mismatched articles.
- Generated summaries are optional conveniences and never replace source links.
