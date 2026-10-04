# Content authors and profile publication

`Challenge.creator` and `Subtask.creator` identify the original content author.
They support creator filters, retained ownership checks, imports, exports and
historical records. They remain present for private authors. A creator filter
selects content; `solved`, `attempted` and `rated` belong to the authenticated
learner. The `xp` field on content is its authored reward, not the author's XP.

Content responses contain no author profile projection or profile URL. Do not
enrich creator references through the ordinary identity cache. Content access
does not require the author to share their learner profile. Academy maintenance
does not replace historical authorship; administrators create new content under
their own identity and existing update paths retain the original creator.

Any future person projection or profile link needs the current publication
authority (`academy-verified-v1`), the verified Academy audience, an allowlisted
DTO and a fresh epoch check before output. Known creator UUIDs, bookmarks,
administrator status and client-supplied scope strings grant no public access.
Private and unknown foreign targets use the same refusal. Public rankings
already use this contract in `services/leaderboard/published.rs`; private support
and owner routes remain separate. There is currently no foreign profile route.

The active frontend course pages do not render catalogue author URLs. The
unused legacy course header, which still had raw author links, is removed in
V4. Catalogue author records and external credit URLs themselves are retained.
No new profile link or person projection is introduced. Publication reader/UI
defaults remain false; the separate activation step owns their switch.

`private_content_author_references_postgres` plays private authors' exercises
through real routes in both reader modes. It checks creator filtering, learner
progress isolation, unchanged creator IDs and retained owner reads. The existing
content-authority test also checks that administrator maintenance preserves the
original author. Publication route tests and the platform acceptance runner
cover the separate ranking and service boundaries.
