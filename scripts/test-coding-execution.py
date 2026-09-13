#!/usr/bin/env python3
"""Disposable native PostgreSQL/Redis and process regression; no live services.

Requires cargo, initdb, pg_ctl, psql and redis-server on PATH (e.g. the dev shell).
Keeps logs in the explicitly selected new output directory; removes its own DB.
"""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import tomllib
import urllib.request
import uuid


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait_for(check, timeout=15):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        if check():
            return
        time.sleep(0.1)
    raise AssertionError("local fixture condition timed out")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    work = Path(__file__).resolve().parents[1]
    temp = Path(tempfile.mkdtemp(prefix="academy-coding-execution-"))
    pg_port, redis_port, api_port = free_port(), free_port(), free_port()
    user = "coding_execution_test"
    env = {key: value for key, value in os.environ.items()
           if key in {"PATH", "HOME", "LD_LIBRARY_PATH", "RUSTUP_HOME", "CARGO_HOME"}}
    env["CONFIG_PATH"] = str(work / "config.toml")
    report = {"passed": False, "commands": [], "loopback_only": True}
    processes, files = [], []
    pg_started = False
    release_executor = threading.Event()
    requests = []

    class Executor(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            requests.append(self.path)
            self.rfile.read(int(self.headers.get("Content-Length", "0")))
            release_executor.wait(90)
            try:
                self.send_error(503, "synthetic executor unavailable")
            except (BrokenPipeError, ConnectionResetError):
                pass

    executor = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Executor)
    executor.daemon_threads = True
    executor_thread = threading.Thread(target=executor.serve_forever, daemon=True)
    executor_thread.start()

    def run(name, argv, *, extra=None, stdin=None, timeout=180):
        started = time.monotonic()
        result = subprocess.run(argv, cwd=work, env={**env, **(extra or {})}, input=stdin,
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                timeout=timeout)
        (out / f"{name}.log").write_text(result.stdout)
        report["commands"].append({"name": name, "argv": list(map(str, argv)),
                                   "exit": result.returncode,
                                   "seconds": round(time.monotonic() - started, 3)})
        print(f"{name}: {result.returncode}", flush=True)
        if result.returncode:
            raise RuntimeError(f"{name} failed; see {out / (name + '.log')}")
        return result.stdout

    def sql(db, query):
        return subprocess.check_output(
            ["psql", "-X", "-qAt", "-v", "ON_ERROR_STOP=1", "-h", "127.0.0.1",
             "-p", str(pg_port), "-U", user, "-d", db],
            input=query, text=True, env=env, stderr=subprocess.STDOUT).strip()

    def db_url(db):
        return f"postgresql://{user}@127.0.0.1:{pg_port}/{db}"

    def migrate(db, count=None):
        return run(f"migration-{db}-{'all' if count is None else count}",
                   [str(work / "target/debug/migration"), "up"] +
                   ([] if count is None else ["-n", str(count)]),
                   extra={"DATABASE_URL": db_url(db)})

    def seed(db, complete=False):
        task, subtask, author, learner, submission = [str(uuid.uuid4()) for _ in range(5)]
        sql(db, f"""
            INSERT INTO challenges_tasks(id,creator,creation_timestamp) VALUES('{task}','{author}',now());
            INSERT INTO challenges_subtasks(id,task_id,creator,creation_timestamp,xp,coins,enabled,retired,ty)
              VALUES('{subtask}','{task}','{author}',now(),0,0,true,false,'coding_challenge');
            INSERT INTO challenges_coding_challenges(subtask_id,time_limit,memory_limit,evaluator,description,solution_environment,solution_code,static_tests,random_tests)
              VALUES('{subtask}',1000,128,'synthetic evaluator','local fixture','python','synthetic',1,1);
            INSERT INTO challenges_coding_challenge_submissions(id,subtask_id,creator,creation_timestamp,environment,code,charge_on_failure)
              VALUES('{submission}','{subtask}','{learner}',now(),'python','synthetic learner code',false);
        """)
        if complete:
            sql(db, f"INSERT INTO challenges_coding_challenge_result(submission_id,verdict) VALUES('{submission}','ok')")
        return submission

    def start(name, argv, extra):
        log = (out / f"{name}.log").open("w")
        files.append(log)
        process = subprocess.Popen(argv, cwd=work, env={**env, **extra}, stdout=log, stderr=log)
        processes.append(process)
        return process

    def stop(process):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)

    try:
        run("build", ["cargo", "build", "--locked", "--offline", "-p", "challenges", "-p", "migration"], timeout=600)
        run("initdb", ["initdb", "-D", str(temp / "data"), "--no-locale", "--encoding=UTF8", "-A", "trust", "-U", user])
        run("postgres-start", ["pg_ctl", "-D", str(temp / "data"), "-l", str(out / "postgres.log"),
                               "-o", f"-h 127.0.0.1 -p {pg_port} -k {temp}", "-w", "start"])
        pg_started = True
        redis = start("redis", ["redis-server", "--bind", "127.0.0.1", "--port", str(redis_port),
                                "--save", "", "--appendonly", "no", "--dir", str(temp)], {})
        def redis_ready():
            try:
                with socket.create_connection(("127.0.0.1", redis_port), timeout=0.1):
                    return True
            except OSError:
                assert redis.poll() is None
                return False
        wait_for(redis_ready)

        for db in ("migration_check", "coding_queue", "heart_regression", "export_regression", "benefit_regression", "process_check"):
            sql("postgres", f"CREATE DATABASE {db}")
        migration_count = (work / "migration/src/lib.rs").read_text().count("Box::new(")
        migrate("migration_check", migration_count - 1)
        completed = seed("migration_check", True)
        pending = seed("migration_check")
        before = sql("migration_check", "SELECT id,code,creation_timestamp,charge_on_failure FROM challenges_coding_challenge_submissions ORDER BY id")
        migrate("migration_check")
        assert before == sql("migration_check", "SELECT id,code,creation_timestamp,charge_on_failure FROM challenges_coding_challenge_submissions ORDER BY id")
        assert sql("migration_check", f"SELECT judge_pending,judge_generation FROM challenges_coding_challenge_submissions WHERE id='{completed}'") == "f|0"
        assert sql("migration_check", f"SELECT judge_pending,judge_generation FROM challenges_coding_challenge_submissions WHERE id='{pending}'") == "t|0"
        report["migration_preserves_existing_submissions"] = True

        for db in ("coding_queue", "heart_regression", "export_regression", "process_check"):
            migrate(db)
        test_env = {"HEART_TEST_REDIS_URL": f"redis://127.0.0.1:{redis_port}/0"}
        run("durable-queue", ["cargo", "test", "--locked", "--offline", "-p", "challenges", "coding_durable_execution_postgres", "--", "--ignored", "--test-threads=1"],
            extra={**test_env, "HEART_TEST_DATABASE_URL": db_url("coding_queue")})
        run("heart-and-authority-regression", ["cargo", "test", "--locked", "--offline", "-p", "challenges", "endpoints::", "--", "--ignored", "--skip", "coding_durable_execution_postgres", "--test-threads=1"],
            extra={**test_env, "HEART_TEST_DATABASE_URL": db_url("heart_regression")})
        run("private-export-regression", ["cargo", "test", "--locked", "--offline", "-p", "challenges", "users::export_tests::", "--", "--ignored", "--test-threads=1"],
            extra={"T11_TEST_DATABASE_URL": db_url("export_regression")})
        run("benefit-regression", ["cargo", "test", "--locked", "--offline", "-p", "challenges", "owning_producer_transaction", "--", "--ignored", "--test-threads=1"],
            extra={"BENEFIT_TEST_DATABASE_URL": db_url("benefit_regression")})

        submission = seed("process_check")
        config = tomllib.loads((work / "config.toml").read_text())
        local = {"DATABASE__URL": db_url("process_check"), "JWT_SECRET": "synthetic-local-worker-test", "RUST_LOG": "warn",
                 "CHALLENGES__HOST": "127.0.0.1", "CHALLENGES__PORT": str(api_port),
                 "CHALLENGES__CODING_CHALLENGES__SANDKASTEN_URL": f"http://127.0.0.1:{executor.server_port}/",
                 "CHALLENGES__CODING_CHALLENGES__EXECUTION__LEASE_SECONDS": "6",
                 "CHALLENGES__CODING_CHALLENGES__EXECUTION__POLL_MILLISECONDS": "50",
                 "CHALLENGES__CODING_CHALLENGES__EXECUTION__MAX_EXECUTION_SECONDS": "60"}
        for key in config["redis"]:
            local[f"REDIS__{key.upper()}"] = f"redis://127.0.0.1:{redis_port}/1"
        for key in config["services"]:
            local[f"SERVICES__{key.upper()}"] = f"http://127.0.0.1:{executor.server_port}/unneeded-{key}/"
        binary = str(work / "target/debug/challenges")
        api = start("api-process", [binary, "api"], local)
        def api_ready():
            assert api.poll() is None
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{api_port}/openapi.json", timeout=0.5) as response:
                    return response.status == 200
            except OSError:
                return False
        wait_for(api_ready)
        time.sleep(1)
        generation = lambda: int(sql("process_check", f"SELECT judge_generation FROM challenges_coding_challenge_submissions WHERE id='{submission}'"))
        assert generation() == 0 and requests == [], "API-only start must not execute work"
        worker_a = start("worker-a", [binary, "worker"], local)
        wait_for(lambda: len(requests) == 1)
        worker_b = start("worker-b", [binary, "worker"], local)
        time.sleep(8)  # Longer than a lease: proves the running worker's heartbeat.
        assert worker_a.poll() is None and worker_b.poll() is None
        assert generation() == 1 and requests == ["/run"], "normal concurrent workers duplicated execution"
        assert sql("process_check", "SELECT sum(capacity) FROM challenge_coding_workers WHERE lease_until>now()") == "4"
        stop(worker_a)
        wait_for(lambda: generation() == 2 and len(requests) == 2)
        assert sql("process_check", "SELECT count(*) FROM challenges_coding_challenge_result") == "0"
        assert sql("process_check", "SELECT count(*) FROM challenge_heart_operations") == "0"
        report["process_checks"] = {"api_only": True, "independent_workers": 2, "heartbeat_survived_lease": True,
                                    "normal_executor_calls": 1, "calls_after_worker_loss": 2,
                                    "no_technical_failure_heart_charge": True}
        run("regular-tests", ["cargo", "test", "--locked", "--offline", "--workspace"])
        run("clippy", ["cargo", "clippy", "--locked", "--offline", "--workspace", "--all-targets"])
        run("format", ["cargo", "fmt", "--all", "--check"])
        report["passed"] = True
    finally:
        for process in reversed(processes):
            stop(process)
        release_executor.set()
        executor.shutdown()
        executor.server_close()
        for log in files:
            log.close()
        if pg_started:
            run("postgres-stop", ["pg_ctl", "-D", str(temp / "data"), "-m", "fast", "-w", "stop"])
        shutil.rmtree(temp)
        report["owned_fixtures_stopped_and_removed"] = True
        report["source_sha256"] = {str(p.relative_to(work)): hashlib.sha256(p.read_bytes()).hexdigest()
                                   for p in work.rglob("*.rs") if "target" not in p.parts}
        (out / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({"passed": report["passed"], "report": str(out / "result.json")}), flush=True)


if __name__ == "__main__":
    main()
