LOCAL_PATH := $(call my-dir)

include $(CLEAR_VARS)
LOCAL_MODULE := bass
LOCAL_SRC_FILES := path/to/bass/libs/$(TARGET_ARCH_ABI)/libbass.so
LOCAL_EXPORT_C_INCLUDES := path/to/bass
include $(PREBUILT_SHARED_LIBRARY)

include $(CLEAR_VARS)
LOCAL_MODULE := bassraw
LOCAL_SRC_FILES := ../bassraw.cpp
#LOCAL_CFLAGS += 
#LOCAL_C_INCLUDES += 
LOCAL_SHARED_LIBRARIES := bass
#LOCAL_LDLIBS += -llog
LOCAL_ARM_NEON := false
include $(BUILD_SHARED_LIBRARY)
