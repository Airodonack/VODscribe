# Reporting a security problem

If you find a vulnerability in VODscribe, please report it privately rather than in a public issue, so a fix can ship before the details are out: use [GitHub's private vulnerability reporting](https://github.com/Airodonack/VODscribe/security/advisories/new) for this repository.

Expect a reply within a few days. Fixes go out as a normal release, which installed copies offer to update to; the release notes credit the reporter unless they'd rather not be named.

VODscribe runs on your computer and talks only to the sites it fetches VODs from, its own download server (`dl.vodscribe.com`) and the projects it downloads tools from; every download is checked against a pinned SHA-256 or a signature before use. The most useful reports concern those paths: a way to make the app fetch or run something it shouldn't, or to read data outside its own folders.
