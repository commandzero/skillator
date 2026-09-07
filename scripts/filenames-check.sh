#!/bin/bash
# Source this file to reuse the check, or pass directories to run it directly.
check_lowercase_filenames() {
  if [[ "$#" -eq 0 ]]; then
    echo 'Usage: check_lowercase_filenames DIRECTORY ...' >&2
    return 2
  fi
  local inventory status path name failed=0
  local LC_ALL=C
  inventory=$(mktemp) || return 1
  # NUL delimiters preserve spaces and newlines; do not hide scan errors in an if pipeline.
  if rg --files --hidden --no-ignore --null -- "$@" > "$inventory"; then
    status=0
  else
    status=$?
  fi
  if [[ "$status" -gt 1 ]]; then
    rm -f "$inventory"
    return "$status"
  fi
  while IFS= read -r -d '' path; do
    name=${path##*/}
    if [[ "$name" == *[A-Z]* ]]; then
      printf 'Filename must be lowercase: %s\n' "$path" >&2
      failed=1
    fi
  done < "$inventory"
  rm -f "$inventory"
  return "$failed"
}
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  check_lowercase_filenames "$@"
fi
