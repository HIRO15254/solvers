# Variant-token helpers sourced by buckets.sh and flop_eval.sh (relative-reach/gcp/job.sh repeats them inline).
# hand_spec [mN][d][pN][f][q][b][g] | c[N] -> --l1-hand (f = binary persistent flop-draws, q = pairs,
# b = backdoor, g = gutshot; b and g need d). hm2dp2 -> made:2,draws,potential:2, hc -> category,
# hdf -> draws,flop-draws, hdq -> draws,pairs, hdqbg -> draws,pairs,backdoor,gutshot.
hand_spec() {
  if [[ $1 =~ ^c([0-9]*)$ ]]; then
    echo "category${BASH_REMATCH[1]:+:${BASH_REMATCH[1]}}"
    return
  fi
  [[ $1 =~ ^(m([0-9]+))?(d)?(p([0-9]+))?(f)?(q)?(b)?(g)?$ && -n $1 ]] || return 1
  local hs=
  [[ -n ${BASH_REMATCH[2]} ]] && hs=made:${BASH_REMATCH[2]}
  [[ -n ${BASH_REMATCH[3]} ]] && hs=${hs:+$hs,}draws
  [[ -n ${BASH_REMATCH[5]} ]] && hs=${hs:+$hs,}potential:${BASH_REMATCH[5]}
  [[ -n ${BASH_REMATCH[6]} ]] && hs=${hs:+$hs,}flop-draws
  [[ -n ${BASH_REMATCH[7]} ]] && hs=${hs:+$hs,}pairs
  [[ -n ${BASH_REMATCH[8]} ]] && hs=${hs:+$hs,}backdoor
  [[ -n ${BASH_REMATCH[9]} ]] && hs=${hs:+$hs,}gutshot
  echo "$hs"
}
# runout_spec LETTERS -> the --l1-runout value: f flush, p pair, s straight, o over (rofp -> flush,pair).
runout_spec() {
  local spec=$1 rs= k
  [[ -n $spec ]] || return 1
  for ((k = 0; k < ${#spec}; k++)); do
    case ${spec:k:1} in f) rs+=flush, ;; p) rs+=pair, ;; s) rs+=straight, ;; o) rs+=over, ;; *) return 1 ;; esac
  done
  echo "${rs%,}"
}

# lsprMAXrNxLgK -> low-SPR threshold MAX, NxL relative buckets, K flop groups.
# The second argument is the group directory (cloud inputs are uploaded flat).
low_spr_spec() {
  [[ $1 =~ ^lspr([0-9]+([.][0-9]+)?)r([0-9]+x[0-9]+)g([0-9]+)$ ]] || return 1
  echo "${BASH_REMATCH[1]}:${BASH_REMATCH[3]}:$2/k${BASH_REMATCH[4]}-groups.txt"
}
