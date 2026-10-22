# Pauses this Mac's pnxfarm worker while a long run uses the Mac, so that no build takes its
# cores and skews the result, and resumes it when the run ends (PNXStudios/pnxfarm,
# docs/using-pnxfarm.md). The mac-*.sh scripts source it. It does nothing on a Mac without
# pnxfarm, and only warns if pausing fails, so a run never stops because of the farm.
#
#   source "$(dirname "${BASH_SOURCE[0]}")/pnxfarm-pause.sh"
#   farm_pause "SPRT of PR #42"   # waits for a build the worker has in hand to finish
#   ...and call farm_resume from the script's EXIT trap.

FARM_PAUSED=""

farm_pause() {
  command -v pnxfarm >/dev/null || return 0
  if pnxfarm pause --reason "$1" --wait; then
    FARM_PAUSED=1
  else
    echo "warning: couldn't pause this Mac's pnxfarm worker; a build may share the Mac" >&2
  fi
}

farm_resume() {
  [[ -n "$FARM_PAUSED" ]] || return 0
  pnxfarm resume >/dev/null || echo "warning: run 'pnxfarm resume' to give the Mac back to the farm" >&2
  FARM_PAUSED=""
}
