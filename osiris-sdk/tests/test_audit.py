"""Denetim zinciri (audit_logs) testleri — sahte DBAPI bağlantısıyla."""

from osiris.audit import append_audit, verify_chain


class FakeCursor:
    def __init__(self, rows=None, has_version_column=False):
        self.rows = list(rows or [])
        self.has_version_column = has_version_column
        self.executed = []
        self.inserts = []
        self._last = ""

    def execute(self, query, params=None):
        self.executed.append(query)
        self._last = query
        if query.strip().upper().startswith("INSERT"):
            self.inserts.append(params)

    def fetchone(self):
        if "information_schema" in self._last:
            return (1,) if self.has_version_column else None
        return self.rows[0] if self.rows else None

    def fetchall(self):
        return self.rows

    def close(self):
        pass


class FakeConn:
    def __init__(self, rows=None, has_version_column=False):
        self.cur = FakeCursor(rows, has_version_column)
        self.committed = False

    def cursor(self):
        return self.cur

    def commit(self):
        self.committed = True


def test_append_audit_genesis() -> None:
    conn = FakeConn(rows=[])
    out = append_audit(conn, "api", "search", None, {"q": "x"})
    assert out["prev_hash"] == "GENESIS"
    assert len(out["hash"]) == 64
    assert conn.committed
    assert conn.cur.inserts[0][1] == "search"


def test_append_audit_chains_prev_hash() -> None:
    conn = FakeConn(rows=[("abc123",)])
    out = append_audit(conn, "api", "collect", "rss", {})
    assert out["prev_hash"] == "abc123"
    assert conn.cur.inserts[0][4] == "abc123"


def test_append_audit_rejects_bad_action() -> None:
    conn = FakeConn()
    try:
        append_audit(conn, None, "")
    except ValueError:
        pass
    else:
        raise AssertionError("boş action kabul edildi")
    assert not conn.cur.inserts


def test_verify_chain_ok_and_tamper() -> None:
    c1 = FakeConn()
    h1 = append_audit(c1, "u", "a1", "r", {"n": 1})["hash"]
    c2 = FakeConn(rows=[(h1,)])
    h2 = append_audit(c2, "u", "a2", None, None)["hash"]

    def row(rid, user, action, resource, detail, prev, digest):
        return (rid, user, action, resource, detail, prev, digest)

    good = FakeConn(rows=[
        row(2, "u", "a2", "", {}, h1, h2),
        row(1, "u", "a1", "r", {"n": 1}, None, h1),
    ])
    res = verify_chain(good)
    assert res["ok"] is True and res["checked"] == 2

    tampered = FakeConn(rows=[
        row(2, "u", "a2", "", {}, h1, h2),
        row(1, "u", "a1", "r", {"n": 999}, None, h1),
    ])
    res2 = verify_chain(tampered)
    assert res2["ok"] is False


def test_append_audit_truncates_before_hash() -> None:
    # 200+ karakter user_id: saklananla hash girdisi aynı olmalı (tekrar doğrulanabilir)
    conn = FakeConn()
    long_user = "u" * 300
    out = append_audit(conn, long_user, "act", "res", {})
    assert conn.cur.inserts[0][0] == "u" * 200
    # verify kanonik yükü saklanan değerden kurar → tutarlı olmalı
    check = FakeConn(rows=[(1, "u" * 200, "act", "res", {}, None, out["hash"])])
    assert verify_chain(check)["ok"] is True


# ---- chain_version 2 (JCS) ----------------------------------------------------

from osiris.audit import (  # noqa: E402
    canonical_payload,
    chain_hash,
    verify_records,
)


def test_v2_written_when_column_exists() -> None:
    conn = FakeConn(rows=[], has_version_column=True)
    out = append_audit(conn, "api", "search", None, {"q": "x"})
    assert out["chain_version"] == 2
    assert conn.cur.inserts[0][-1] == 2
    assert out["hash"] == chain_hash(
        None, canonical_payload("api", "search", "", {"q": "x"}, 2))


def test_v1_fallback_without_migration() -> None:
    conn = FakeConn(rows=[], has_version_column=False)
    out = append_audit(conn, "api", "search", None, {"q": "x"})
    assert out["chain_version"] == 1
    assert len(conn.cur.inserts[0]) == 6  # chain_version sütunu yazılmaz


def test_jcs_number_forms_survive_jsonb_roundtrip() -> None:
    # jsonb 1.0'ı "1.0", 1e16'yı 10000000000000000 olarak geri verir; v1 bunu
    # farklı serileştirirdi, JCS aynı sayı = aynı dizge garanti eder
    assert canonical_payload("u", "a", "", {"f": 1.0, "big": 1e16}, 2) == \
        canonical_payload("u", "a", "", {"f": 1, "big": 10000000000000000}, 2)
    assert canonical_payload("u", "a", "", {"f": 1.0}, 1) != \
        canonical_payload("u", "a", "", {"f": 1}, 1)


def test_unsafe_ints_and_nan_normalized() -> None:
    conn = FakeConn(rows=[], has_version_column=True)
    append_audit(conn, "u", "a", None, {"n": 2**60, "ok": 5})
    import json
    stored = json.loads(conn.cur.inserts[0][3])
    assert stored == {"n": str(2**60), "ok": 5}
    conn2 = FakeConn(rows=[], has_version_column=True)
    append_audit(conn2, "u", "a", None, {"x": float("nan")})
    assert json.loads(conn2.cur.inserts[0][3]) == {"_unserializable": True}


def test_mixed_v1_v2_chain_verifies() -> None:
    p1 = canonical_payload("u", "a1", "r", {"n": 1}, 1)
    h1 = chain_hash(None, p1)
    p2 = canonical_payload("u", "a2", "", {"f": 1.0}, 2)
    h2 = chain_hash(h1, p2)
    rows = [
        (1, "u", "a1", "r", {"n": 1}, None, h1, 1),
        (2, "u", "a2", "", '{"f": 1.0}', h1, h2, 2),
    ]
    assert verify_records(rows) == {"ok": True, "checked": 2}
    # sürüm etiketi değiştirilirse (v2 kayıt v1 sayılırsa) hash tutmaz
    forged = [rows[0], (*rows[1][:7], 1)]
    assert verify_records(forged)["ok"] is False
    assert verify_records([(1, "u", "a", "", {}, None, "x", 9)])["ok"] is False


def test_verify_chain_reads_version_column() -> None:
    p = canonical_payload("u", "a", "", {}, 2)
    h = chain_hash(None, p)
    conn = FakeConn(rows=[(1, "u", "a", "", {}, None, h, 2)], has_version_column=True)
    assert verify_chain(conn)["ok"] is True
    assert "chain_version" in conn.cur.executed[1]
