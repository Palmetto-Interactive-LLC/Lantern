# How to Cut a Release

Lantern releases are published from signed, annotated `v*` tags. The release workflow refuses lightweight tags and tags whose signature GitHub cannot verify.

## Prerequisites

Maintainers need SSH commit/tag signing configured locally:

```bash
git config --global gpg.format ssh
git config --global commit.gpgsign true
git config --global tag.gpgsign true
git config --global user.signingkey ~/.ssh/<github-signing-key>.pub
git config --global gpg.ssh.allowedSignersFile ~/.ssh/allowed_signers
```

`~/.ssh/allowed_signers` must map the Git email used in this repository to the public signing key:

```text
you@example.com namespaces="git" ssh-ed25519 AAAA...
```

Do not put private keys in `allowed_signers`.

The same public key must also be uploaded to GitHub as an **SSH signing key** for the releasing account. An SSH authentication key alone is not enough; the release workflow checks GitHub's tag-signature verification result before it builds assets.

Verify the local signing setup before cutting a release:

```bash
make release-signing-smoke
```

## Release Steps

Start from a clean, current `main`:

```bash
git switch main
git pull --ff-only
git status --short --branch
```

Run the gates:

```bash
make verify
make security
make package-smoke
make install-smoke
make release-signing-smoke
```

Bump `Cargo.toml` and `Cargo.lock`, merge the version bump through a pull request, then tag the merge commit:

```bash
VERSION=2026.8.0
TAG="v${VERSION}"
git switch main
git pull --ff-only
git tag -s -m "Lantern ${TAG}" "$TAG"
git tag -v "$TAG"
git push origin "refs/tags/${TAG}"
```

Always pass `-m`. With `tag.gpgsign=true`, plain `git tag "$TAG"` enters the signed annotated tag path and may block waiting for an editor.

Watch the release workflow:

```bash
gh run list --workflow Release --limit 1
gh run watch <run-id>
gh release view "$TAG" --json tagName,name,isDraft,isPrerelease,publishedAt,url,assets
```

Smoke-test the published installer from a temporary home:

```bash
tmp_home="$(mktemp -d)"
HOME="$tmp_home" sh -c 'curl -fsSL https://raw.githubusercontent.com/Palmetto-Interactive-LLC/Lantern/main/scripts/install.sh | sh'
"$tmp_home/.lantern/bin/lantern" --version
rm -rf "$tmp_home"
```

## Recovery

If a tag was created incorrectly and the release workflow rejected it before publishing a release, delete the local and remote tag, then recreate it as a signed annotated tag:

```bash
git tag -d "$TAG"
git push origin ":refs/tags/${TAG}"
git tag -s -m "Lantern ${TAG}" "$TAG"
git tag -v "$TAG"
git push origin "refs/tags/${TAG}"
```

Do not delete or retarget a tag after a GitHub Release has been published unless there is a documented security or legal reason.
