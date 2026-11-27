# Herdr fork CI

- The fork's source-packaging mode keeps upstream usage reporting off. Give the app its own GitHub Releases address because `HERDR_RELEASES_URL` also redirects Core requests.
- The update version includes the CI run number. A Cargo version alone misses feature merges between upstream releases. Keep workflow run numbers increasing and use `fork-N` tags for this fork.
- Publish from the fork's default branch after both native builds pass. Upload and verify the two archives and their metadata while the Release is a draft. An older run or a commit the branch has passed must not replace Latest.
