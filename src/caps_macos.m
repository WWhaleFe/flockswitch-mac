// Raw Caps Lock edges arrive before input-source flagsChanged, including long holds.
#import <Foundation/Foundation.h>
#import <IOKit/hid/IOHIDManager.h>
#import <IOKit/hid/IOHIDUsageTables.h>
#import <mach/mach_time.h>
#include <stdlib.h>

typedef struct {
    IOHIDManagerRef manager;
    CFMutableSetRef held;
    void (*callback)(void *, bool, uint64_t);
    void *context;
    mach_timebase_info_data_t timebase;
} FSCaps;

static void caps_value(void *context, IOReturn result, void *sender, IOHIDValueRef value) {
    (void)sender;
    if (result != kIOReturnSuccess) return;
    FSCaps *caps = context;
    IOHIDElementRef element = IOHIDValueGetElement(value);
    if (IOHIDElementGetUsagePage(element) != kHIDPage_KeyboardOrKeypad ||
        IOHIDElementGetUsage(element) != kHIDUsage_KeyboardCapsLock) return;
    IOHIDDeviceRef device = IOHIDElementGetDevice(element);
    bool before = CFSetGetCount(caps->held) != 0;
    if (IOHIDValueGetIntegerValue(value)) CFSetAddValue(caps->held, device);
    else CFSetRemoveValue(caps->held, device);
    bool down = CFSetGetCount(caps->held) != 0;
    if (before != down) {
        uint64_t now = mach_absolute_time(), at = IOHIDValueGetTimeStamp(value);
        uint64_t age = at <= now ? (uint64_t)((long double)(now - at) * caps->timebase.numer / caps->timebase.denom) : 0;
        caps->callback(caps->context, down, age);
    }
}

static void caps_removed(void *context, IOReturn result, void *sender, IOHIDDeviceRef device) {
    (void)result; (void)sender;
    FSCaps *caps = context;
    bool before = CFSetGetCount(caps->held) != 0;
    CFSetRemoveValue(caps->held, device);
    if (before && !CFSetGetCount(caps->held)) caps->callback(caps->context, false, 0);
}

void *fs_caps_start(void (*callback)(void *, bool, uint64_t), void *context) {
    FSCaps *caps = calloc(1, sizeof(*caps));
    if (!caps) return NULL;
    caps->manager = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
    caps->held = CFSetCreateMutable(kCFAllocatorDefault, 0, &kCFTypeSetCallBacks);
    caps->callback = callback;
    caps->context = context;
    mach_timebase_info(&caps->timebase);
    NSDictionary *matching = @{@kIOHIDDeviceUsagePageKey: @(kHIDPage_GenericDesktop),
                              @kIOHIDDeviceUsageKey: @(kHIDUsage_GD_Keyboard)};
    IOHIDManagerSetDeviceMatching(caps->manager, (__bridge CFDictionaryRef)matching);
    IOHIDManagerRegisterInputValueCallback(caps->manager, caps_value, caps);
    IOHIDManagerRegisterDeviceRemovalCallback(caps->manager, caps_removed, caps);
    IOHIDManagerScheduleWithRunLoop(caps->manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
    if (IOHIDManagerOpen(caps->manager, kIOHIDOptionsTypeNone) != kIOReturnSuccess) {
        IOHIDManagerUnscheduleFromRunLoop(caps->manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
        CFRelease(caps->manager);
        CFRelease(caps->held);
        free(caps);
        return NULL;
    }
    return caps;
}

void fs_caps_stop(void *handle) {
    FSCaps *caps = handle;
    IOHIDManagerUnscheduleFromRunLoop(caps->manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
    IOHIDManagerClose(caps->manager, kIOHIDOptionsTypeNone);
    CFRelease(caps->manager);
    CFRelease(caps->held);
    free(caps);
}
