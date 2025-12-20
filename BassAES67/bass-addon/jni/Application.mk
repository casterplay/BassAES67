APP_ABI := arm64-v8a armeabi-v7a x86 x86_64
APP_CFLAGS += -flto -fsigned-char -fvisibility=hidden -ffast-math
APP_LDFLAGS += -Wl,--gc-sections -Wl,--as-needed  -Wl,--warn-shared-textrel -Wl,--fatal-warnings -Wl,-Bsymbolic
APP_LDFLAGS += -Wl,-z,max-page-size=16384  # 16k pages
