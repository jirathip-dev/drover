# Hosts: lanes on more than one machine (#157)

One **main host** runs drover, the board (`drover serve`) and the event log. It starts lanes on any host over ssh
(shed there through `shed connect`), picks the least-busy host that fits, and shows every host on the board. With no `[hosts]` in
fleet.toml nothing changes: every lane runs on the main host, as before.

Each section is a file in docs/hosts/.
