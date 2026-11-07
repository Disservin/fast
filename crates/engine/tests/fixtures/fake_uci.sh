#!/bin/sh
# A tiny UCI engine for tests. `go infinite` waits for `stop`; `go depth` answers at once.
crash_on_go=$1
while IFS= read -r line; do
  case "$line" in
    uci)
      echo "id name FakeFish 1.0"
      echo "id author Tests"
      echo "option name Hash type spin default 16 min 1 max 1024"
      echo "option name Marker type string default <empty>"
      echo "uciok"
      ;;
    "setoption name Marker value "*)
      marker=${line#setoption name Marker value }
      ;;
    "position "*)
      position=$line
      ;;
    "go infinite")
      [ "$crash_on_go" = crash ] && exit 3
      echo "info depth 10 multipv 1 score cp 25 wdl 400 400 200 nodes 100 nps 1000 string ignored"
      echo "info depth 12 multipv 1 score cp ${marker:-30} wdl 410 400 190 nodes 200 nps 2000 pv e2e4 e7e5"
      echo "fake engine warning" >&2
      searching=1
      ;;
    "go "*)
      echo "info depth 1 score cp ${marker:-0} pv d2d4"
      echo "bestmove d2d4 ponder d7d5"
      ;;
    stop)
      if [ -n "$searching" ]; then
        # A stale line arriving after stop must be dropped by the supervisor.
        echo "info depth 13 multipv 1 score cp 99 pv a2a3"
        echo "bestmove e2e4"
        searching=
      fi
      ;;
    quit)
      exit 0
      ;;
  esac
done
