"""Publish both fork CI archives as a single GitHub Release."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib


TARGETS = ("windows-x86_64", "macos-aarch64")


def gh(*args):
    return subprocess.check_output(["gh", *args], text=True).strip()


def publish():
    repo = os.environ["GITHUB_REPOSITORY"]
    commit = os.environ["GITHUB_SHA"]
    branch = os.environ["GITHUB_REF_NAME"]
    build = int(os.environ["GITHUB_RUN_NUMBER"])
    tag = f"fork-{build}"
    repo_url = f"{os.environ['GITHUB_SERVER_URL']}/{repo}"
    api = f"repos/{repo}"

    def branch_head():
        return gh("api", f"{api}/git/ref/heads/{branch}", "--jq", ".object.sha")

    if branch_head() != commit:
        print(f"Skipping {tag}: {branch} has moved past {commit}.")
        return

    # A retry must neither change a published release nor move Latest backwards.
    releases = gh(
        "api", "--paginate", f"{api}/releases?per_page=100",
        "--jq", '.[] | select(.tag_name | test("^fork-[1-9][0-9]*$")) | {tag_name, draft, target_commitish}',
    )
    records = [json.loads(line) for line in releases.splitlines()]
    if any(not record["draft"] and int(record["tag_name"].removeprefix("fork-")) >= build
           for record in records):
        print(f"Skipping {tag}: this or a newer build is already published.")
        return

    draft = next((record for record in records if record["tag_name"] == tag), None)
    if draft and draft["target_commitish"] != commit:
        raise RuntimeError(f"Draft {tag} targets a different commit.")

    with Path("Cargo.toml").open("rb") as cargo:
        version = f"{tomllib.load(cargo)['package']['version']}+fork.{build}"
    dist = Path("dist")
    targets = {}
    files = {}
    for target in TARGETS:
        archive = dist / f"herdr-app-{target}.tar.gz"
        files[archive] = hashlib.sha256(archive.read_bytes()).hexdigest()
        targets[target] = {"url": f"{repo_url}/releases/download/{tag}/{archive.name}", "sha256": files[archive]}
    sums = dist / "SHA256SUMS"
    sums.write_text("".join(f"{sha}  {path.name}\n" for path, sha in files.items()), encoding="ascii")
    manifest = dist / "manifest.json"
    manifest.write_text(json.dumps({"version": version, "targets": targets}, indent=2) + "\n", encoding="utf-8")
    for path in (sums, manifest):
        files[path] = hashlib.sha256(path.read_bytes()).hexdigest()

    if draft is None:
        build_url = f"{repo_url}/actions/runs/{os.environ['GITHUB_RUN_ID']}"
        print(gh(
            "release", "create", tag, "--repo", repo, "--target", commit, "--draft",
            "--title", f"Herdr personal fork {version}",
            "--notes", (
                f"Windows x64 and macOS arm64 builds from [`{commit[:7]}`]({repo_url}/commit/{commit}). "
                f"[CI run]({build_url}) · [Installation notes]({repo_url}#personal-fork)"
            ),
        ))
    gh("release", "upload", tag, *(str(path) for path in files), "--repo", repo, "--clobber")

    release = json.loads(gh(
        "release", "view", tag, "--repo", repo, "--json", "databaseId,isDraft,targetCommitish",
    ))
    if not release["isDraft"] or release["targetCommitish"] != commit:
        raise RuntimeError(f"Release {tag} is no longer the expected draft.")
    assets = json.loads(gh("api", f"{api}/releases/{release['databaseId']}/assets"))
    uploaded = {asset["name"]: asset["digest"] for asset in assets if asset["state"] == "uploaded"}
    expected = {path.name: f"sha256:{digest}" for path, digest in files.items()}
    if uploaded != expected:
        raise RuntimeError(f"Release {tag} has missing, unexpected, or different assets.")
    if branch_head() != commit:
        print(f"Keeping {tag} as a draft: {branch} advanced during the upload.")
        return
    print(gh("release", "edit", tag, "--repo", repo, "--draft=false", "--latest"))


if __name__ == "__main__":
    publish()
