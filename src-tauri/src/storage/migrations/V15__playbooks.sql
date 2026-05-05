-- V15__playbooks.sql
-- Structured playbook artifact. One playbook per (date, account, generation_id).
-- Multiple generation_ids per date allowed so an intraday refresh hook can be
-- added later without migration; v1 only writes one generation per day.

CREATE TABLE playbooks (
    date            TEXT    NOT NULL,        -- "YYYY-MM-DD" (ET)
    account         TEXT    NOT NULL,
    generation_id   INTEGER NOT NULL,        -- monotonic per (date, account)
    generated_at    TEXT    NOT NULL,        -- ISO 8601 UTC
    ranked_setups   TEXT    NOT NULL,        -- JSON array of RankedSetup
    skip_list       TEXT    NOT NULL,        -- JSON array of SkipEntry
    llm_call_id     TEXT,
    PRIMARY KEY (date, account, generation_id)
);

CREATE INDEX idx_playbooks_date ON playbooks(date);
CREATE INDEX idx_playbooks_account_date_gen
    ON playbooks(account, date DESC, generation_id DESC);
