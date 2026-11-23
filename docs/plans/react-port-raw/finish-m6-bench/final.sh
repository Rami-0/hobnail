#!/bin/bash
# Final bench: one container per scenario, so memory.peak is per scenario.
cd "$(dirname "$0")"; IMG=$1; O=final; mkdir -p $O
sc() { name=$1; shift; TIGHT=200 ./bench-guards.sh "$IMG" $O/$name "$@" > $O/$name.log 2>&1; sleep 1
  echo "== $name: $(grep memory.peak $O/$name.log) tight[$(grep -E '^(anon|sock|file) ' $O/$name/tight.txt | tr '\n' ' ')]"; grep -E '^\| [a-z0-9-]+ \|' $O/$name.log | grep -v -- '-after'; grep '^  - ' $O/$name.log | head -3; }
sc clients-5ip clients:40:none
CLIENT_BOXES=25 sc clients-25ip clients:40:none
APP_ENV="-e RH_MAX_CONNECTIONS=128 -e RH_MAX_CONNECTIONS_PER_PEER=32" sc clients-5ip-at128 clients:40:none
APP_ENV="-e RH_MAX_CONNECTIONS=128 -e RH_MAX_CONNECTIONS_PER_PEER=32" CLIENT_BOXES=25 sc clients-25ip-at128 clients:40:none
sc header-1ip header-1ip:40:header/50/1
sc body-1ip body-1ip:40:body/50/1
sc noread-1ip noread-1ip:40:noread/30/1
sc pipe401-1ip pipe401-1ip:40:pipe401/16/1
sc pipe401-4ip pipe401-4ip:40:pipe401/16/4
sc pipe401-8ip pipe401-8ip:40:pipe401/16/8
sc mixed-5ip mixed-5ip:40:header/50/1,body/50/1,noread/30/1,pipe401/16/2
echo DONE
