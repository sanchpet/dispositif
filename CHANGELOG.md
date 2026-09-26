# Changelog

## 0.1.0 (2026-09-26)


### Features

* **allowlist:** decide admission from a TOML policy before any model runs ([60a94f5](https://github.com/sanchpet/dispositif/commit/60a94f5b97f79d127ebbfccbb5d1187a74b25b5f))
* **mcp:** talk to mcp-tg over short-lived streamable-HTTP sessions ([9998efa](https://github.com/sanchpet/dispositif/commit/9998efaaaf208dad6e934fd2cbfd80fb960fc92b))
* **poll:** watch the agent account and emit admitted messages ([c44baf9](https://github.com/sanchpet/dispositif/commit/c44baf94cf562791d4de991c362d27e9ef911324))
* **runner:** answer admitted messages with one headless claude run each ([8332c58](https://github.com/sanchpet/dispositif/commit/8332c5896c8df29cf417184dbeadf1942435548b))


### Bug Fixes

* **allowlist:** do not count @&lt;agent&gt;_suffix as a mention of the agent ([b7045d6](https://github.com/sanchpet/dispositif/commit/b7045d66724e2b33ad3d1b00fadeec8a4b37402a))
* **config:** allow only read-only tools in a restricted tier ([2123fbb](https://github.com/sanchpet/dispositif/commit/2123fbb9efd74a003fd5a82ae9d5ec38e65705e2))
* **config:** reject settings check passed but runs ignored or broke on ([32f27c7](https://github.com/sanchpet/dispositif/commit/32f27c748cbbfff0e0d694f51a593b9b479628f4))
* **mcp:** read SSE responses event by event ([72c2346](https://github.com/sanchpet/dispositif/commit/72c234663dd840d7080f686b0d48e593c416e79c))
* **poll:** never answer a message twice when marking it read fails ([eb97ec1](https://github.com/sanchpet/dispositif/commit/eb97ec1df1e8e0fc89c9f0ceddb0c6d613927884))
* **poll:** persist state even when a poll cycle fails ([8c916a9](https://github.com/sanchpet/dispositif/commit/8c916a9f2755c2ea0e0e0a8edb573724ec4508cb))
* **runner:** kill everything a run started when it ends or times out ([bf46533](https://github.com/sanchpet/dispositif/commit/bf465336a4fea055dbdffdd9f609467b2845bf95))
* **runner:** mark history from senders outside the allowlist ([0bb69e9](https://github.com/sanchpet/dispositif/commit/0bb69e944624b73e90ac5895b05aa21f3388af98))
* **runner:** start a fresh session after a failed run ([fef284d](https://github.com/sanchpet/dispositif/commit/fef284d754f6726420bb7f0bd47c673bd1793d45))
