// File chores AppKit does better than plain Rust: moving a file to the Trash.

#import <Foundation/Foundation.h>
#include <stdbool.h>

/// Moves the file at `path` (UTF-8) to the Trash of its volume. False when that cannot
/// be done, for example on a volume without a Trash.
bool fs_move_to_trash(const char *path) {
    @autoreleasepool {
        NSString *p = path ? [NSString stringWithUTF8String:path] : nil;
        if (!p) return false;
        NSError *error = nil;
        BOOL ok = [NSFileManager.defaultManager trashItemAtURL:[NSURL fileURLWithPath:p] resultingItemURL:nil error:&error];
        if (!ok) NSLog(@"flockswitch: cannot move %@ to the Trash: %@", p, error);
        return ok;
    }
}
