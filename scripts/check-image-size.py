#!/usr/bin/env python3
"""Gate a local Docker image tag on the exact byte count shown by docker images."""

import json
import os
import subprocess
import sys

LIMIT = 13_500_000


def main():
    if len(sys.argv) != 2:
        sys.exit(f"usage: {sys.argv[0]} <image-tag>")
    image = sys.argv[1]
    endpoint = os.environ.get("DOCKER_HOST", "")
    if os.environ.get("DOCKER_CONTEXT") or not endpoint:
        endpoint = subprocess.check_output(
            ["docker", "context", "inspect", "--format", "{{.Endpoints.docker.Host}}"],
            text=True,
        ).strip()
    if not endpoint.startswith("unix://"):
        sys.exit("image size check requires a local Docker Unix socket")

    # Inspect .Size can mean compressed content on a containerd store.
    # The image list endpoint supplies the same metric as docker images.
    filters = json.dumps({"reference": [image]})
    data = subprocess.check_output(
        [
            "curl", "-fsS", "--unix-socket", endpoint.removeprefix("unix://"),
            "--get", "--data-urlencode", f"filters={filters}",
            "http://localhost/images/json",
        ],
        text=True,
    )
    images = json.loads(data)
    if len(images) != 1:
        sys.exit(f"{image} must match exactly one local image")
    size = images[0]["Size"]
    if not isinstance(size, int) or size < 1:
        sys.exit(f"{image} returned an invalid size")
    print(f"{image}: {size:,} B (limit {LIMIT:,} B)")
    return int(size > LIMIT)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (subprocess.CalledProcessError, ValueError, KeyError) as error:
        sys.exit(f"image size check failed: {error}")
