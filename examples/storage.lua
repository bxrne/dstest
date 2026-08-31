--- @diagnostic disable:undefined-global
--- Virtual disk fault injection: write a file, snapshot it, corrupt the disk,
--- observe the damage, restore from snapshot.
--- Run (requires root): cat examples/storage.lua | cargo run
---
--- Storage faults need the host loop/device-mapper path, so this example is
--- opt-in at subject creation via the `storage` setup field.

local cfg = dstest.config({
    substrate = "docker",
    seed = 0x570A6,
    accumulation = "single",
})

-- The disk is prepared and bind-mounted at /data before the container starts.
local s = dstest.setup(cfg, {
    image = "alpine:3.20",
    cmd = { "sleep", "300" },
    storage = { flaky = true, mount = "/data", size_mb = 32 },
})

local function write_ledger(content)
    -- Atomically replace the ledger on the virtual disk.
    local w = dstest.exec(s, { "sh", "-c", "printf '%s' '" .. content .. "' > /data/ledger.txt && sync" })
    assert(w.exit_code == 0, "write failed: " .. w.stderr)
end

local function read_ledger()
    local r = dstest.exec(s, { "cat", "/data/ledger.txt" })
    return r.exit_code == 0 and r.stdout or nil
end

-- 1. Write a known-good ledger and confirm it persists.
write_ledger("balance=100; ok")
assert(read_ledger() == "balance=100; ok", "ledger should be intact after write")

-- 2. Snapshot the healthy backing store.
local snap = dstest.storage.snapshot(s)
dstest.info("snapshot taken: " .. tostring(snap))

-- 3. Corrupt the disk (seeded, deterministic offsets) and observe the damage.
dstest.storage.corrupt(s, 8)
local damaged = read_ledger()
dstest.info("after corrupt, ledger = " .. tostring(damaged))
assert(damaged ~= "balance=100; ok", "corruption must have changed the disk contents")

-- 4. Restore from the snapshot taken before corruption.
dstest.storage.restore(s, snap)
local restored = read_ledger()
dstest.info("after restore, ledger = " .. tostring(restored))
assert(restored == "balance=100; ok", "restore must bring back the original bytes")

-- 5. Toggle I/O errors: writes should fail while EIO is injected.
dstest.storage.error(s, true)
local eio = dstest.exec(s, { "sh", "-c", "echo x > /data/failing.txt" })
dstest.info("write under EIO exit_code=" .. tostring(eio.exit_code))
dstest.storage.error(s, false)

-- After clearing errors the disk works again.
write_ledger("balance=200; ok")
assert(read_ledger() == "balance=200; ok", "disk must accept writes after EIO cleared")

dstest.dst.clear(s)
dstest.info("storage example complete")
