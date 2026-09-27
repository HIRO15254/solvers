"""Repeat the three GET inventories retaining warnings/unreachable scopes too."""
import json
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

import collect


def main():
    record = {"schema": "r1.usage-inventory-check/v1", "started_at": collect.now(), "requests": [],
              "source": collect.pin(__import__("pathlib").Path(__file__).read_bytes()), "cloud_mutations": False}
    deadline = time.monotonic() + 100
    token = None
    try:
        auth = subprocess.run([sys.executable, str(collect.SDK), "auth", "print-access-token", "--quiet"], capture_output=True, timeout=30)
        record["authentication_exit_code"] = auth.returncode
        if auth.returncode:
            raise RuntimeError("authentication unavailable")
        token = auth.stdout.decode("ascii").strip()
        if not token or any(c.isspace() for c in token):
            raise ValueError("unexpected authentication output")
        for resource in ("instances", "disks", "addresses"):
            params = {"maxResults": "500", "fields": f"items/*/{resource}(id,name),items/*/warning,warning,unreachables,nextPageToken"}
            if resource != "addresses":
                params["filter"] = "name eq solvers-r1-.*"
            url = f"https://compute.googleapis.com/compute/v1/projects/{collect.PROJECT}/aggregated/{resource}?" + urllib.parse.urlencode(params)
            entry = {"resource": resource, "url": url, "method": "GET", "requested_at": collect.now()}
            record["requests"].append(entry)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError()
            request = urllib.request.Request(url, headers={"Authorization": "Bearer " + token})
            try:
                response = urllib.request.urlopen(request, timeout=min(20, remaining))
            except urllib.error.HTTPError as error:
                response = error
            with response:
                data = response.read(1024 * 1024 + 1)
                if len(data) > 1024 * 1024:
                    raise ValueError("response size bound")
                entry.update(http_status=response.status, response=collect.save("inventory-" + resource + ".json", data), completed_at=collect.now())
            parsed = json.loads(data)
            if entry["http_status"] != 200 or parsed.get("nextPageToken") or parsed.get("unreachables"):
                raise ValueError("inventory incomplete")
            warnings = [parsed.get("warning")] + [scope.get("warning") for scope in parsed.get("items", {}).values()]
            if any(w and w.get("code") != "NO_RESULTS_ON_PAGE" for w in warnings):
                raise ValueError("nonempty inventory warning")
            entry["returned_count"] = sum(len(scope.get(resource, [])) for scope in parsed.get("items", {}).values())
            entry["no_unreachable_scopes"] = True
        record["status"] = "completed"
    except Exception as error:
        record.update(status="incomplete", error_type=type(error).__name__)
    finally:
        token = None
        record["ended_at"] = collect.now()
        collect.save("inventory-check.json", (json.dumps(record, indent=2) + "\n").encode())
    print(json.dumps({"status": record["status"], "counts": [r.get("returned_count") for r in record["requests"]]}))
    return int(record["status"] != "completed")


if __name__ == "__main__":
    raise SystemExit(main())
