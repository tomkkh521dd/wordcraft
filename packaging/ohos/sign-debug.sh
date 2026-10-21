#!/usr/bin/env bash
# Signs a HAP without DevEco Studio, using the SDK's hap-sign-tool.
#
#   packaging/ohos/sign-debug.sh --material DIR IN.hap OUT.hap
#       DIR holds the developer's signing material:
#         password.txt          the keystore/key password (one line)
#         something.p12         the keystore (the app's key pair)
#         something.cer         the app's certificate chain (leaf, CA, root) in PEM
#         something.p7b         the provisioning profile for this bundle (debug)
#       The .cer and .p7b come from AppGallery Connect (or DevEco's "Automatically generate
#       signature"); the profile must name the bundle and, for a debug app, the device's UDID.
#       WORDCRAFT_KEY_ALIAS overrides the key alias (the first one in the keystore is used).
#
#   packaging/ohos/sign-debug.sh --openharmony-test IN.hap OUT.hap
#       Signs with the test material the SDK ships (OpenHarmony.p12 and its profile keys), good for
#       an OpenHarmony device or emulator. A HarmonyOS image rejects it ("install sign info
#       inconsistent"): it trusts only Huawei's chain, so use --material there. WORDCRAFT_UDID (or
#       `hdc` answering `bm get --udid`) supplies the device a debug profile is bound to.
#
# The material never goes into this repository, and no password is written to disk.
set -euo pipefail

. "$(dirname "$0")/env.sh"

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

mode=""
material=""
args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --material)
      mode=material
      material="${2:-}"
      [ -n "$material" ] || {
        echo "error: --material needs a directory" >&2
        exit 2
      }
      shift 2
      ;;
    --openharmony-test)
      mode=test
      shift
      ;;
    -h | --help) usage 0 ;;
    -*)
      echo "error: unknown option $1" >&2
      exit 2
      ;;
    *)
      args+=("$1")
      shift
      ;;
  esac
done
[ -n "$mode" ] || usage 2
[ "${#args[@]}" -eq 2 ] || {
  echo "error: expected IN.hap and OUT.hap" >&2
  exit 2
}
in_hap="${args[0]}"
out_hap="${args[1]}"
[ -f "$in_hap" ] || {
  echo "error: no such HAP: $in_hap" >&2
  exit 1
}

SIGN_LIB="$OHOS_NDK/../toolchains/lib"
[ -f "$SIGN_LIB/hap-sign-tool.jar" ] || SIGN_LIB="$OHOS_NDK/../../../toolchains/lib"
TOOL="$SIGN_LIB/hap-sign-tool.jar"
[ -f "$TOOL" ] || {
  echo "error: hap-sign-tool.jar not found under $SIGN_LIB" >&2
  exit 1
}
if [ -x "/c/Program Files/Huawei/DevEco Studio/jbr/bin/java.exe" ]; then
  JAVA="/c/Program Files/Huawei/DevEco Studio/jbr/bin/java.exe"
elif [ -x "/Applications/DevEco-Studio.app/Contents/jbr/Contents/Home/bin/java" ]; then
  JAVA="/Applications/DevEco-Studio.app/Contents/jbr/Contents/Home/bin/java"
else
  JAVA="java"
fi
KEYTOOL="$(dirname "$JAVA")/keytool"
[ -x "$KEYTOOL.exe" ] && KEYTOOL="$KEYTOOL.exe"
[ -x "$KEYTOOL" ] || KEYTOOL="keytool"

# The API level the HAP claims (compatibleSdkVersion in ohos/build-profile.json5: 6.1.1(24) → 24).
compatible_version=24

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The first key alias in a keystore, so --material does not have to guess it.
first_key_alias() {
  "$KEYTOOL" -J-Duser.language=en -list -keystore "$1" -storepass "$2" -storetype PKCS12 2>/dev/null |
    grep "PrivateKeyEntry" | head -1 | sed 's/,.*//'
}

if [ "$mode" = "test" ]; then
  keystore="$SIGN_LIB/OpenHarmony.p12"
  store_pwd=123456
  key_pwd=123456
  app_alias="openharmony application release"
  profile_alias="openharmony application profile debug"
  template="$SIGN_LIB/UnsgnedDebugProfileTemplate.json"

  udid="${WORDCRAFT_UDID:-}"
  if [ -z "$udid" ] && command -v hdc >/dev/null 2>&1; then
    udid="$(hdc shell bm get --udid 2>/dev/null | tail -1 | tr -d '\r\n ')"
  fi
  [ -n "$udid" ] || {
    echo "error: --openharmony-test needs the device UDID (WORDCRAFT_UDID, or hdc on PATH)" >&2
    exit 1
  }
  # A fresh debug profile for this bundle, valid now: the shipped template expired in 2024 and
  # names OpenHarmony's demo bundle.
  python3 - "$template" "$work/profile.json" "$work/app-cert.pem" "$udid" <<'PY'
import json, sys, time, uuid
template, out, cert_out, udid = sys.argv[1:5]
t = json.load(open(template))
t["uuid"] = str(uuid.uuid4())
now = int(time.time())
t["validity"] = {"not-before": now - 86400, "not-after": now + 365 * 86400}
t["bundle-info"]["bundle-name"] = "karl.hmsoft.wordcraft"
t["debug-info"]["device-ids"] = [udid]
json.dump(t, open(out, "w"), indent=1)
open(cert_out, "w").write(t["bundle-info"]["development-certificate"])
PY
  # The app's certificate chain: the leaf the profile names, then the CA and the root from the
  # keystore (the keystore's own release entry holds a self-signed cert, not this chain).
  {
    cat "$work/app-cert.pem"
    "$KEYTOOL" -J-Duser.language=en -exportcert -rfc -alias "openharmony application ca" -keystore "$keystore" -storepass "$store_pwd" -storetype PKCS12 2>/dev/null
    "$KEYTOOL" -J-Duser.language=en -exportcert -rfc -alias "openharmony application root ca" -keystore "$keystore" -storepass "$store_pwd" -storetype PKCS12 2>/dev/null
  } > "$work/app-chain.cer"
  app_cert="$work/app-chain.cer"

  "$JAVA" -jar "$TOOL" sign-profile -mode localSign -keyAlias "$profile_alias" -keyPwd "$key_pwd" \
    -profileCertFile "$SIGN_LIB/OpenHarmonyProfileDebug.pem" -inFile "$work/profile.json" \
    -signAlg SHA256withECDSA -keystoreFile "$keystore" -keystorePwd "$store_pwd" \
    -outFile "$work/profile.p7b" >/dev/null
  profile="$work/profile.p7b"
else
  [ -d "$material" ] || {
    echo "error: no such directory: $material" >&2
    exit 1
  }
  [ -f "$material/password.txt" ] || {
    echo "error: $material/password.txt is missing" >&2
    exit 1
  }
  keystore="$(find "$material" -maxdepth 1 -name '*.p12' -type f | head -1)"
  app_cert="$(find "$material" -maxdepth 1 -name '*.cer' -type f | head -1)"
  profile="$(find "$material" -maxdepth 1 -name '*.p7b' -type f | head -1)"
  for f in "$keystore" "$app_cert" "$profile"; do
    if [ -z "$f" ] || [ ! -f "$f" ]; then
      echo "error: $material needs a .p12, a .cer (certificate chain) and a .p7b (profile)" >&2
      echo "       found: $(ls "$material" | tr '\n' ' ')" >&2
      exit 1
    fi
  done
  store_pwd="$(tr -d '\r\n' < "$material/password.txt")"
  key_pwd="$store_pwd"
  app_alias="${WORDCRAFT_KEY_ALIAS:-$(first_key_alias "$keystore" "$store_pwd")}"
  [ -n "$app_alias" ] || {
    echo "error: no key entry found in $keystore: is password.txt the keystore password?" >&2
    exit 1
  }
fi

"$JAVA" -jar "$TOOL" sign-app -mode localSign -keyAlias "$app_alias" -keyPwd "$key_pwd" \
  -appCertFile "$app_cert" -profileFile "$profile" -inFile "$in_hap" -signAlg SHA256withECDSA \
  -keystoreFile "$keystore" -keystorePwd "$store_pwd" -outFile "$out_hap" \
  -compatibleVersion "$compatible_version" >/dev/null

echo "WordCraft: signed $out_hap (key alias: $app_alias)"
echo "Install it with: hdc install -r \"$out_hap\""
