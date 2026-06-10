# Merill Product Context

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

## Features

**Feed**
The main view. Stories are grouped into clusters and can be filtered by local (Malta) or global publishers. Four sort modes are available: Balanced (For You), Latest, Covered, and Blindspots. Stories can be searched, saved, or dismissed. A "New" badge marks stories that arrived since the user's last session.

**Fil-Qosor (In Brief)**
Quick AI-generated summaries of the day's top stories, one per cluster. Designed for scanning without opening individual articles.

**Blindspots**
A filtered view showing only clusters where no independent publisher is represented — stories covered solely by state-owned, party-owned, or church-owned media.

**Story detail**
Opens a cluster to show all sources, a combined summary, a bias coverage bar, and a perspective breakdown by ownership group. Includes a timeline of when each publisher first reported the story. Individual articles can be read in full with adjustable font size, line spacing, and translated/original text toggle.

**Settings**
- Theme (system/light/dark) and feed language (English/Maltese)
- Per-publisher enable/disable toggles for both local and global feeds
- Publisher bias category overrides (user can correct ownership classification)
- Custom publisher addition via RSS or website URL
- Advanced: force re-cluster, wipe all local data

## Terminology

- **Story group / cluster:** Articles Merill believes describe the same event.
- **Perspective:** One publisher's report within a story group.
- **Ownership category (local):** State-owned, party-owned (PL), party-owned (PN), church-owned, commercial independent, investigative independent.
- **Ownership category (global):** Left, centre, right — describes editorial leaning rather than legal ownership.
- **Blindspot:** A story group with coverage but no independent publisher represented.
- **Balanced (sort mode):** Feed ordering weighted by freshness, number of publishers, and presence of independent coverage.
- **Fil-Qosor:** Maltese for "in brief" — the quick-summary tab.

## Trust Boundaries

- Merill does not rate factual truth.
- Publisher ownership metadata may be corrected by the user.
- Automated grouping can be wrong; users can remove mismatched articles.
- Generated summaries are optional conveniences and never replace source links.
