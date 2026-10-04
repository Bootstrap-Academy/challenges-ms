#!/usr/bin/env python3
"""Run daily-access and existing heart/queue regressions on disposable databases.

Requires cargo, initdb, pg_ctl, psql and redis-server on PATH. Every HTTP peer is
a loopback fixture; no real users, live executor or external service is used.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    root = Path(__file__).resolve().parents[1]
    temp = Path(tempfile.mkdtemp(prefix="academy-learning-access-"))
    env = {key: value for key, value in os.environ.items() if key in {
        "PATH", "HOME", "LD_LIBRARY_PATH", "RUSTUP_HOME", "CARGO_HOME", "CARGO_TARGET_DIR"
    }}
    env["CONFIG_PATH"] = str(root / "config.toml")
    target = Path(env.get("CARGO_TARGET_DIR", root / "target")).resolve()
    pg_port, redis_port = free_port(), free_port()
    user = "learning_access_test"
    report = {"passed": False, "loopback_only": True, "commands": []}
    pg_started = False
    redis = None

    def run(name, argv, extra=None, stdin=None):
        started = time.monotonic()
        with (out / f"{name}.log").open("w") as log:
            result = subprocess.run(argv, cwd=root, env={**env, **(extra or {})},
                                    input=stdin, text=True, stdout=log,
                                    stderr=subprocess.STDOUT, timeout=600)
        report["commands"].append({"name": name, "argv": list(map(str, argv)),
                                   "exit": result.returncode,
                                   "seconds": round(time.monotonic() - started, 3)})
        print(f"{name}: {result.returncode}", flush=True)
        result.check_returncode()

    try:
        run("migration-build", ["cargo", "build", "--locked", "--offline", "-j", "2", "-p", "migration"])
        run("initdb", ["initdb", "-D", str(temp / "data"), "--no-locale", "--encoding=UTF8", "-A", "trust", "-U", user])
        run("postgres-start", ["pg_ctl", "-D", str(temp / "data"), "-l", str(out / "postgres.log"),
                               "-o", f"-h 127.0.0.1 -p {pg_port} -k {temp}", "-w", "start"])
        pg_started = True
        with (out / "redis.log").open("w") as log:
            redis = subprocess.Popen(["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port),
                                      "--save", "", "--appendonly", "no", "--dir", str(temp)],
                                     env=env, stdout=log, stderr=log)
        deadline = time.monotonic() + 15
        while True:
            try:
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.1):
                    break
            except OSError:
                if redis.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError("Local Redis failed to start")
                time.sleep(0.1)
        for db, filters in [
            ("access_regression", ["endpoints::", "--", "--ignored", "--skip", "coding_durable_execution_postgres", "--skip", "coding_inline_", "--skip", "publication_", "--test-threads=1"]),
            ("queue_regression", ["coding_durable_execution_postgres", "--", "--ignored", "--test-threads=1"]),
            ("inline_regression", ["coding_inline_", "--", "--ignored", "--test-threads=1"]),
            ("publication_regression", ["publication_", "--", "--ignored", "--test-threads=1"]),
        ]:
            run(f"create-{db}", ["psql", "-X", "-v", "ON_ERROR_STOP=1", "-h", "127.0.0.1", "-p", str(pg_port),
                                 "-U", user, "-d", "postgres"], stdin=f"CREATE DATABASE {db}")
            db_url = f"postgresql://{user}@127.0.0.1:{pg_port}/{db}"
            run(f"migrate-{db}", [str(target / "debug/migration"), "up"], {"DATABASE_URL": db_url})
            run(db, ["cargo", "test", "--locked", "--offline", "-j", "2", "-p", "challenges", *filters],
                {"HEART_TEST_DATABASE_URL": db_url, "HEART_TEST_REDIS_URL": f"redis://127.0.0.1:{redis_port}/0",
                 "PRIV01_TEST_DATABASE_URL": db_url, "PRIV01_TEST_REDIS_URL": f"redis://127.0.0.1:{redis_port}/0"})
        report["passed"] = True
    finally:
        if redis is not None:
            redis.terminate()
            try:
                redis.wait(timeout=10)
            except subprocess.TimeoutExpired:
                redis.kill()
                redis.wait(timeout=10)
        if pg_started:
            run("postgres-stop", ["pg_ctl", "-D", str(temp / "data"), "-m", "immediate", "-w", "stop"])
        shutil.rmtree(temp)
        report["cleanup_complete"] = True
        (out / "result.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
