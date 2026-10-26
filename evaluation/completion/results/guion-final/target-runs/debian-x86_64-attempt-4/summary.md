# Debian 12 x86_64 release-target verification

Source revision: `03e78deb7d0e9d4f8b559005c3d4be75d326ab38`

All ten individual target gates passed, including the release gate. All 18 application reports passed. The verification script then failed while assembling its final metadata because `git rev-parse HEAD` could not read Git metadata inside the Debian container.

**Overall: FAILED during evidence assembly. This attempt does not validate x86_64.**
