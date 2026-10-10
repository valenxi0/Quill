# Contributing to Quill

Bug reports, documentation improvements, and focused pull requests are welcome.

For a bug, include your Quill version, Windows version, selected engine, and steps to reproduce. Share only audio or transcripts you are comfortable making public. For a larger feature, open an issue to discuss the approach first.

## Working on the app

Follow [Development](docs/DEVELOPMENT.md) to install prerequisites and build Quill. To preview the UI with sample data:

```powershell
npm ci
npm run dev
# Open http://localhost:1420/?onboarded
```

Before submitting a pull request, run the checks relevant to your change:

```powershell
npm run build
cargo test -p quill-core --locked
```

Keep changes focused, describe the behavior you changed, and include screenshots for UI changes. Preserve model licenses and attribution. Cleanup changes should follow the [cleanup contract](docs/CLEANUP.md) and retain the existing guards and model evaluation rubrics.
