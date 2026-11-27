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
