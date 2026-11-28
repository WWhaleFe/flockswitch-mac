// Korean/English switching for a Mac that is controlled: select the input source directly
// instead of replaying Caps Lock (macOS times Caps Lock itself and may keep a capital lock).
#import <Foundation/Foundation.h>
#import <Carbon/Carbon.h>
#import <dispatch/dispatch.h>

static BOOL source_is_korean(TISInputSourceRef source) {
    CFArrayRef languages = TISGetInputSourceProperty(source, kTISPropertyInputSourceLanguages);
    if (!languages) return NO;
    for (CFIndex i = 0; i < CFArrayGetCount(languages); i++) {
        CFStringRef language = CFArrayGetValueAtIndex(languages, i);
        if (CFStringHasPrefix(language, CFSTR("ko"))) return YES;
    }
    return NO;
}

// The first enabled Korean keyboard input source, or NULL (the caller releases it).
static TISInputSourceRef find_korean(void) {
    NSDictionary *filter = @{(__bridge NSString *)kTISPropertyInputSourceType: (__bridge NSString *)kTISTypeKeyboardInputMode};
    CFArrayRef modes = TISCreateInputSourceList((__bridge CFDictionaryRef)filter, false);
    NSDictionary *layouts = @{(__bridge NSString *)kTISPropertyInputSourceType: (__bridge NSString *)kTISTypeKeyboardLayout};
    CFArrayRef plain = TISCreateInputSourceList((__bridge CFDictionaryRef)layouts, false);
    TISInputSourceRef found = NULL;
    CFArrayRef lists[2] = {modes, plain};
    for (int k = 0; k < 2 && !found; k++) {
        if (!lists[k]) continue;
        for (CFIndex i = 0; i < CFArrayGetCount(lists[k]); i++) {
            TISInputSourceRef source = (TISInputSourceRef)CFArrayGetValueAtIndex(lists[k], i);
            CFBooleanRef selectable = TISGetInputSourceProperty(source, kTISPropertyInputSourceIsSelectCapable);
            if (selectable && CFBooleanGetValue(selectable) && source_is_korean(source)) {
                found = (TISInputSourceRef)CFRetain(source);
                break;
            }
        }
    }
    if (modes) CFRelease(modes);
    if (plain) CFRelease(plain);
    return found;
}

static TISInputSourceRef previous_latin = NULL;

static void toggle(void) {
    TISInputSourceRef current = TISCopyCurrentKeyboardInputSource();
    if (!current) return;
    if (source_is_korean(current)) {
        // Back to the Latin source used before, else the latest ASCII-capable one.
        TISInputSourceRef target = previous_latin ? (TISInputSourceRef)CFRetain(previous_latin) : TISCopyCurrentASCIICapableKeyboardInputSource();
        if (target) {
            TISSelectInputSource(target);
            CFRelease(target);
        }
    } else {
        TISInputSourceRef korean = find_korean();
        if (korean) {
            if (previous_latin) CFRelease(previous_latin);
            previous_latin = (TISInputSourceRef)CFRetain(current);
            TISSelectInputSource(korean);
            CFRelease(korean);
        }
    }
    CFRelease(current);
}

// Text Input Sources calls belong on the main queue; the network thread does not wait.
void fs_input_source_toggle(void) {
    dispatch_async(dispatch_get_main_queue(), ^{ toggle(); });
}

// --- the characters of a key that is put in (a controlled Mac) ------------------------
// A key event made by CGEventCreateKeyboardEvent carries the characters of the key without
// the modifiers set on it afterwards: Shift+Q carries "q". Apps work the characters out
// again, but an input method such as 2-Set Korean uses the ones carried, so Shift+Q typed
// ㅂ instead of ㅃ (and ㄲ ㄸ ㅆ ㅉ ㅒ ㅖ were never typed). The receiver therefore writes the
// characters the current keyboard layout gives for the key with its modifiers, as a real
// keyboard does.
#include <os/lock.h>

static CFDataRef layout_data = NULL;
static os_unfair_lock layout_lock = OS_UNFAIR_LOCK_INIT;

// Main queue only (Text Input Sources).
static void refresh_layout(void) {
    TISInputSourceRef source = TISCopyCurrentKeyboardLayoutInputSource();
    CFDataRef data = source ? TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) : NULL;
    if (data) CFRetain(data);
    os_unfair_lock_lock(&layout_lock);
    CFDataRef old = layout_data;
    layout_data = data;
    os_unfair_lock_unlock(&layout_lock);
    if (old) CFRelease(old);
    if (source) CFRelease(source);
}

static void layout_changed(CFNotificationCenterRef center, void *observer, CFNotificationName name,
                           const void *object, CFDictionaryRef info) {
    (void)center; (void)observer; (void)name; (void)object; (void)info;
    refresh_layout();
}

// Reads the current keyboard layout and follows its changes. Safe to call more than once.
void fs_key_layout_watch(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        static BOOL watching = NO;
        if (!watching) {
            watching = YES;
            CFNotificationCenterAddObserver(CFNotificationCenterGetDistributedCenter(), NULL, layout_changed,
                kTISNotifySelectedKeyboardInputSourceChanged, NULL, CFNotificationSuspensionBehaviorDeliverImmediately);
        }
        refresh_layout();
    });
}

// The characters `keycode` types with CGEvent `flags`, written to `out`; their count, or -1
// when the layout is not known yet or the key is a dead key (its event is left as it is).
int fs_key_chars(uint16_t keycode, uint64_t flags, uint16_t *out, int max) {
    os_unfair_lock_lock(&layout_lock);
    CFDataRef data = layout_data ? (CFDataRef)CFRetain(layout_data) : NULL;
    os_unfair_lock_unlock(&layout_lock);
    if (!data) return -1;
    UInt32 modifiers = 0;
    if (flags & kCGEventFlagMaskCommand) modifiers |= cmdKey >> 8;
    if (flags & kCGEventFlagMaskShift) modifiers |= shiftKey >> 8;
    if (flags & kCGEventFlagMaskAlphaShift) modifiers |= alphaLock >> 8;
    if (flags & kCGEventFlagMaskAlternate) modifiers |= optionKey >> 8;
    if (flags & kCGEventFlagMaskControl) modifiers |= controlKey >> 8;
    UInt32 dead = 0;
    UniCharCount length = 0;
    OSStatus status = UCKeyTranslate((const UCKeyboardLayout *)CFDataGetBytePtr(data), keycode, kUCKeyActionDown,
        modifiers, LMGetKbdType(), 0, &dead, (UniCharCount)max, &length, (UniChar *)out);
    CFRelease(data);
    if (status != noErr || dead != 0) return -1;
    return (int)length;
}
