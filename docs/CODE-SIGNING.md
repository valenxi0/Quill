# Code signing policy

Quill has applied to the [SignPath Foundation](https://signpath.org) for free code signing, and the application is under review. Once approved, Windows releases are signed with a certificate issued to SignPath Foundation, and free code signing is provided by [SignPath.io](https://signpath.io). Until then, releases are not code-signed.

## What gets signed

- `Quill.exe`, the application.
- `Quill-Setup.exe` and `Quill_<version>_x64-setup.exe`, the installer.
- The ONNX Runtime and sherpa-onnx DLLs that the installer places next to `Quill.exe`.

The Visual C++ runtime DLLs are already signed by Microsoft. Model weights are not part of the installer; Quill downloads them from Hugging Face during setup and checks each file against a pinned SHA-256 checksum.

## How releases are built

Every release is built from this repository by the public [Release workflow](../.github/workflows/release.yml) on GitHub-hosted runners, when a version tag such as `v0.3.0` is pushed. Nothing is built or signed on a personal machine. Each signing request needs manual approval from an approver before the build is signed.

## Team roles

Quill has one maintainer, who holds every role:

| Role | Members |
| --- | --- |
| Authors (commit without review) | [valenxi0](https://github.com/valenxi0) |
| Reviewers (review outside contributions) | [valenxi0](https://github.com/valenxi0) |
| Approvers (approve each signing request) | [valenxi0](https://github.com/valenxi0) |

Pull requests from other contributors are reviewed before they are merged. Every team member uses multi-factor authentication for GitHub and SignPath.

## Privacy

Quill does not send audio, transcripts, or other personal data anywhere. Recognition and cleanup run on the user's PC. Quill connects to the internet only to:

- download the speech models the user chooses, from Hugging Face;
- check GitHub once a day for a newer Quill release, and download it when the user chooses Update.

No analytics or telemetry are collected. Insights and transcript history stay on the user's PC.
