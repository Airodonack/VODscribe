#!/usr/bin/env python3
"""Uploads staged models (see stage.py) to the `vodscribe` R2 bucket under models/.

usage: CLOUDFLARE_ACCOUNT_ID=... uv run --with boto3 publish.py DIST

Reads a Cloudflare API token with R2 write access from
~/.config/cloudflare/r2_token (or $CLOUDFLARE_R2_TOKEN_FILE) and derives S3
credentials from it, as Cloudflare documents: access key id = the token's id,
secret = SHA-256 of the token's value. Large files go up in multipart uploads.

Published files must never change, since released apps pin them by hash (the
bucket also has a lock on models/). A file that is already in the bucket is
skipped if its content matches and refused if it doesn't: ship changed models
under a new model id.
"""

import hashlib
import json
import os
import sys
import urllib.request
from pathlib import Path

import boto3
from boto3.s3.transfer import TransferConfig
from botocore.exceptions import ClientError

BUCKET = "vodscribe"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def remote_sha256(s3, key: str) -> str | None:
    """The object's SHA-256, or None if it doesn't exist. Uses the hash stored
    at upload when present, and otherwise hashes the object's content."""
    try:
        head = s3.head_object(Bucket=BUCKET, Key=key)
    except ClientError as e:
        if e.response["Error"]["Code"] in ("404", "NoSuchKey", "NotFound"):
            return None
        raise
    if stored := head.get("Metadata", {}).get("sha256"):
        return stored
    h = hashlib.sha256()
    for chunk in s3.get_object(Bucket=BUCKET, Key=key)["Body"].iter_chunks(1 << 20):
        h.update(chunk)
    return h.hexdigest()


def main() -> None:
    dist = Path(sys.argv[1])
    account = os.environ["CLOUDFLARE_ACCOUNT_ID"]
    token_file = os.environ.get("CLOUDFLARE_R2_TOKEN_FILE", "~/.config/cloudflare/r2_token")
    token = Path(token_file).expanduser().read_text().strip()

    verify = urllib.request.Request(
        f"https://api.cloudflare.com/client/v4/accounts/{account}/tokens/verify",
        headers={"Authorization": f"Bearer {token}"},
    )
    token_id = json.load(urllib.request.urlopen(verify))["result"]["id"]
    s3 = boto3.client(
        "s3",
        endpoint_url=f"https://{account}.r2.cloudflarestorage.com",
        aws_access_key_id=token_id,
        aws_secret_access_key=hashlib.sha256(token.encode()).hexdigest(),
        region_name="auto",
    )
    config = TransferConfig(multipart_threshold=64 << 20, multipart_chunksize=64 << 20, max_concurrency=8)

    for path in sorted(p for p in dist.rglob("*") if p.is_file()):
        key = f"models/{path.relative_to(dist).as_posix()}"
        local = sha256_file(path)
        remote = remote_sha256(s3, key)
        if remote == local:
            print(f"unchanged {key}", flush=True)
            continue
        if remote is not None:
            sys.exit(f"{key} is already published with different content; ship changes under a new model id")
        content_type = "text/plain; charset=utf-8" if path.suffix == ".txt" else "application/octet-stream"
        extra = {"ContentType": content_type, "Metadata": {"sha256": local}}
        s3.upload_file(str(path), BUCKET, key, ExtraArgs=extra, Config=config)
        size = s3.head_object(Bucket=BUCKET, Key=key)["ContentLength"]
        if size != path.stat().st_size:
            sys.exit(f"{key}: uploaded {size} bytes, expected {path.stat().st_size}")
        print(f"uploaded {key} ({size} bytes)", flush=True)


if __name__ == "__main__":
    main()
