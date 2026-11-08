// CoreBluetooth GATT transport for Logitech HID++ over Bluetooth LE on macOS.
//
// Bolt-generation Logitech devices (e.g. MX Master 4, MX Master 3S, MX Mechanical,
// MX Keys S) and Bluetooth devices blocked by macOS Input Monitoring TCC on IOHIDDevice
// expose HID++ 2.0 on their vendor BLE GATT service:
//   Service UUID:        00010000-0000-1000-8000-011f2000046d
//   Characteristic UUID: 00010001-0000-1000-8000-011f2000046d

#import <Foundation/Foundation.h>
#import <CoreBluetooth/CoreBluetooth.h>
#include <stdint.h>
#include <stdbool.h>
#include <string.h>

static NSString *const kLogitechHidppServiceUUID = @"00010000-0000-1000-8000-011f2000046d";
static NSString *const kDeviceInfoServiceUUID    = @"180A";
static NSString *const kBatteryServiceUUID       = @"180F";
static NSString *const kHidServiceUUID           = @"1812";
static NSString *const kLogitechBleServiceUUID   = @"FE61";
static NSString *const kPnpIdCharUUID            = @"2A50";

@interface FsBleEndpoint : NSObject <CBPeripheralDelegate>
@property (nonatomic, strong) CBPeripheral *peripheral;
@property (nonatomic, strong) CBCharacteristic *hidppChar;
@property (nonatomic, strong) CBCharacteristic *pnpChar;
@property (nonatomic, strong) dispatch_semaphore_t connectSem;
@property (nonatomic, strong) dispatch_semaphore_t serviceSem;
@property (nonatomic, strong) dispatch_semaphore_t charSem;
@property (nonatomic, strong) dispatch_semaphore_t notifySem;
@property (nonatomic, strong) dispatch_semaphore_t pnpSem;
@property (nonatomic, strong) dispatch_semaphore_t writeSem;
@property (nonatomic, strong) dispatch_semaphore_t readSem;
@property (nonatomic, strong) NSMutableArray<NSData *> *reports;
@property (nonatomic, assign) uint16_t vendorId;
@property (nonatomic, assign) uint16_t productId;
@property (nonatomic, assign) int pendingCharServices;
@property (nonatomic, assign) bool notifyOk;
@property (nonatomic, assign) bool writeOk;
@property (nonatomic, assign) bool disconnected;
@property (nonatomic, assign) bool closed;
@end

@implementation FsBleEndpoint

- (instancetype)initWithPeripheral:(CBPeripheral *)peripheral {
    self = [super init];
    if (self) {
        _peripheral = peripheral;
        _peripheral.delegate = self;
        _connectSem = dispatch_semaphore_create(0);
        _serviceSem = dispatch_semaphore_create(0);
        _charSem = dispatch_semaphore_create(0);
        _notifySem = dispatch_semaphore_create(0);
        _pnpSem = dispatch_semaphore_create(0);
        _writeSem = dispatch_semaphore_create(0);
        _readSem = dispatch_semaphore_create(0);
        _reports = [NSMutableArray array];
    }
    return self;
}

- (void)peripheral:(CBPeripheral *)peripheral didDiscoverServices:(NSError *)error {
    (void)peripheral;
    (void)error;
    dispatch_semaphore_signal(self.serviceSem);
}

- (void)peripheral:(CBPeripheral *)peripheral
didDiscoverCharacteristicsForService:(CBService *)service
             error:(NSError *)error {
    (void)peripheral;
    if (!error) {
        CBUUID *hidppServiceUUID = [CBUUID UUIDWithString:kLogitechHidppServiceUUID];
        CBUUID *devInfoUUID = [CBUUID UUIDWithString:kDeviceInfoServiceUUID];
        CBUUID *pnpUUID = [CBUUID UUIDWithString:kPnpIdCharUUID];
        if ([service.UUID isEqual:hidppServiceUUID]) {
            for (CBCharacteristic *c in service.characteristics) {
                CBCharacteristicProperties props = c.properties;
                bool canNotify = (props & (CBCharacteristicPropertyNotify | CBCharacteristicPropertyIndicate)) != 0;
                bool canWrite = (props & (CBCharacteristicPropertyWrite | CBCharacteristicPropertyWriteWithoutResponse)) != 0;
                if (canNotify && canWrite) {
                    self.hidppChar = c;
                    break;
                }
            }
        } else if ([service.UUID isEqual:devInfoUUID]) {
            for (CBCharacteristic *c in service.characteristics) {
                if ([c.UUID isEqual:pnpUUID]) {
                    self.pnpChar = c;
                    break;
                }
            }
        }
    }
    @synchronized (self) {
        self.pendingCharServices -= 1;
        if (self.pendingCharServices <= 0) {
            dispatch_semaphore_signal(self.charSem);
        }
    }
}

- (void)peripheral:(CBPeripheral *)peripheral
didUpdateNotificationStateForCharacteristic:(CBCharacteristic *)characteristic
             error:(NSError *)error {
    (void)peripheral;
    if (characteristic == self.hidppChar) {
        self.notifyOk = (error == nil && characteristic.isNotifying);
        dispatch_semaphore_signal(self.notifySem);
    }
}

- (void)peripheral:(CBPeripheral *)peripheral
didWriteValueForCharacteristic:(CBCharacteristic *)characteristic
             error:(NSError *)error {
    (void)peripheral;
    if (characteristic == self.hidppChar) {
        self.writeOk = (error == nil);
        dispatch_semaphore_signal(self.writeSem);
    }
}

- (void)peripheral:(CBPeripheral *)peripheral
didUpdateValueForCharacteristic:(CBCharacteristic *)characteristic
             error:(NSError *)error {
    (void)peripheral;
    if (error) {
        if (characteristic == self.pnpChar) {
            dispatch_semaphore_signal(self.pnpSem);
        }
        return;
    }
    NSData *data = characteristic.value;
    if (characteristic == self.pnpChar) {
        if (data && data.length >= 5) {
            const uint8_t *bytes = (const uint8_t *)data.bytes;
            self.vendorId = (uint16_t)bytes[1] | ((uint16_t)bytes[2] << 8);
            self.productId = (uint16_t)bytes[3] | ((uint16_t)bytes[4] << 8);
        }
        dispatch_semaphore_signal(self.pnpSem);
    } else if (characteristic == self.hidppChar && data && data.length > 0) {
        NSData *copy = [NSData dataWithBytes:data.bytes length:data.length];
        @synchronized (self.reports) {
            [self.reports addObject:copy];
        }
        dispatch_semaphore_signal(self.readSem);
    }
}

@end

@interface FsBleManager : NSObject <CBCentralManagerDelegate>
@property (nonatomic, strong) dispatch_queue_t queue;
@property (nonatomic, strong) CBCentralManager *central;
@property (nonatomic, strong) dispatch_semaphore_t stateSem;
@property (nonatomic, strong) NSMutableDictionary<NSString *, FsBleEndpoint *> *endpoints;
@property (nonatomic, strong) NSMutableSet<NSString *> *nonLogitech;
@end

@implementation FsBleManager

+ (instancetype)shared {
    static FsBleManager *instance = nil;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{
        instance = [[FsBleManager alloc] init];
    });
    return instance;
}

- (instancetype)init {
    self = [super init];
    if (self) {
        _queue = dispatch_queue_create("io.github.wwhalefe.flockswitch.ble", DISPATCH_QUEUE_SERIAL);
        _stateSem = dispatch_semaphore_create(0);
        _endpoints = [NSMutableDictionary dictionary];
        _nonLogitech = [NSMutableSet set];
        NSDictionary *opts = @{CBCentralManagerOptionShowPowerAlertKey: @NO};
        _central = [[CBCentralManager alloc] initWithDelegate:self queue:_queue options:opts];
    }
    return self;
}

- (void)centralManagerDidUpdateState:(CBCentralManager *)central {
    if (central.state != CBManagerStateUnknown && central.state != CBManagerStateResetting) {
        dispatch_semaphore_signal(self.stateSem);
    }
}

- (void)centralManager:(CBCentralManager *)central didConnectPeripheral:(CBPeripheral *)peripheral {
    (void)central;
    NSString *uuid = peripheral.identifier.UUIDString;
    FsBleEndpoint *ep = nil;
    @synchronized (self.endpoints) {
        ep = self.endpoints[uuid];
    }
    if (ep) {
        dispatch_semaphore_signal(ep.connectSem);
    }
}

- (void)centralManager:(CBCentralManager *)central
didFailToConnectPeripheral:(CBPeripheral *)peripheral
                 error:(NSError *)error {
    (void)central;
    (void)error;
    NSString *uuid = peripheral.identifier.UUIDString;
    FsBleEndpoint *ep = nil;
    @synchronized (self.endpoints) {
        ep = self.endpoints[uuid];
    }
    if (ep) {
        ep.disconnected = true;
        dispatch_semaphore_signal(ep.connectSem);
    }
}

- (void)centralManager:(CBCentralManager *)central
didDisconnectPeripheral:(CBPeripheral *)peripheral
                 error:(NSError *)error {
    (void)central;
    (void)error;
    NSString *uuid = peripheral.identifier.UUIDString;
    FsBleEndpoint *ep = nil;
    @synchronized (self.endpoints) {
        ep = self.endpoints[uuid];
    }
    if (ep) {
        ep.disconnected = true;
        dispatch_semaphore_signal(ep.connectSem);
        dispatch_semaphore_signal(ep.serviceSem);
        dispatch_semaphore_signal(ep.charSem);
        dispatch_semaphore_signal(ep.notifySem);
        dispatch_semaphore_signal(ep.writeSem);
        dispatch_semaphore_signal(ep.readSem);
    }
}

- (bool)waitReady {
    if (self.central.state == CBManagerStatePoweredOn) {
        return true;
    }
    if (self.central.state == CBManagerStateUnknown || self.central.state == CBManagerStateResetting) {
        dispatch_semaphore_wait(self.stateSem, dispatch_time(DISPATCH_TIME_NOW, (int64_t)(1500 * NSEC_PER_MSEC)));
    }
    return self.central.state == CBManagerStatePoweredOn;
}

- (NSArray<CBPeripheral *> *)connectedCandidates {
    if (![self waitReady]) {
        return @[];
    }
    NSMutableDictionary<NSString *, CBPeripheral *> *byId = [NSMutableDictionary dictionary];
    NSArray<NSString *> *serviceStrings = @[
        kLogitechHidppServiceUUID,
        kLogitechBleServiceUUID,
        kDeviceInfoServiceUUID,
        kBatteryServiceUUID,
        kHidServiceUUID,
    ];
    for (NSString *s in serviceStrings) {
        @try {
            CBUUID *uuid = [CBUUID UUIDWithString:s];
            NSArray<CBPeripheral *> *list = [self.central retrieveConnectedPeripheralsWithServices:@[uuid]];
            for (CBPeripheral *p in list) {
                NSString *key = p.identifier.UUIDString;
                if (!key) continue;
                @synchronized (self.nonLogitech) {
                    if ([self.nonLogitech containsObject:key]) continue;
                }
                byId[key] = p;
            }
        } @catch (NSException *exception) {
            (void)exception;
        }
    }
    return byId.allValues;
}

@end

static bool wait_ms(dispatch_semaphore_t sem, int64_t ms) {
    return dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, ms * (int64_t)NSEC_PER_MSEC)) == 0;
}

// Returns newline-separated "ble:<uuid>" keys of currently connected candidate peripherals.
// Caller frees the returned string with `fs_ble_free_string`.
char *fs_ble_connected_keys(void) {
    @autoreleasepool {
        FsBleManager *mgr = [FsBleManager shared];
        NSArray<CBPeripheral *> *peripherals = [mgr connectedCandidates];
        NSMutableArray<NSString *> *keys = [NSMutableArray arrayWithCapacity:peripherals.count];
        for (CBPeripheral *p in peripherals) {
            if (p.identifier.UUIDString) {
                [keys addObject:[NSString stringWithFormat:@"ble:%@", p.identifier.UUIDString.lowercaseString]];
            }
        }
        NSString *joined = [keys componentsJoinedByString:@"\n"];
        return strdup(joined.UTF8String ? joined.UTF8String : "");
    }
}

void fs_ble_free_string(char *s) {
    if (s) {
        free(s);
    }
}

void fs_ble_close(void *handle) {
    if (!handle) return;
    @autoreleasepool {
        FsBleEndpoint *ep = (__bridge_transfer FsBleEndpoint *)handle;
        ep.closed = true;
        ep.disconnected = true;
        NSString *uuid = ep.peripheral.identifier.UUIDString;
        FsBleManager *mgr = [FsBleManager shared];
        @synchronized (mgr.endpoints) {
            if (uuid && mgr.endpoints[uuid] == ep) {
                [mgr.endpoints removeObjectForKey:uuid];
            }
        }
        if (ep.peripheral.state == CBPeripheralStateConnected) {
            if (ep.hidppChar && ep.hidppChar.isNotifying) {
                [ep.peripheral setNotifyValue:NO forCharacteristic:ep.hidppChar];
            }
            [mgr.central cancelPeripheralConnection:ep.peripheral];
        }
    }
}

// Opens a connected BLE peripheral by UUID ("ble:<uuid>" or "<uuid>").
// Returns an opaque handle on success, or NULL if it is not a Logitech HID++ BLE device.
void *fs_ble_open(const char *uuid_cstr, char *name_out, size_t name_cap, uint16_t *product_id_out) {
    if (!uuid_cstr) return NULL;
    @autoreleasepool {
        NSString *raw = [NSString stringWithUTF8String:uuid_cstr];
        if (!raw) return NULL;
        if ([raw hasPrefix:@"ble:"]) {
            raw = [raw substringFromIndex:4];
        }
        NSUUID *nsuuid = [[NSUUID alloc] initWithUUIDString:raw];
        if (!nsuuid) return NULL;

        FsBleManager *mgr = [FsBleManager shared];
        if (![mgr waitReady]) return NULL;

        @synchronized (mgr.nonLogitech) {
            if ([mgr.nonLogitech containsObject:nsuuid.UUIDString]) {
                return NULL;
            }
        }

        NSArray<CBPeripheral *> *peripherals = [mgr.central retrievePeripheralsWithIdentifiers:@[nsuuid]];
        CBPeripheral *peripheral = peripherals.firstObject;
        if (!peripheral) return NULL;

        FsBleEndpoint *ep = [[FsBleEndpoint alloc] initWithPeripheral:peripheral];
        @synchronized (mgr.endpoints) {
            mgr.endpoints[nsuuid.UUIDString] = ep;
        }

        [mgr.central connectPeripheral:peripheral options:nil];
        if (!wait_ms(ep.connectSem, 4000) || ep.disconnected || peripheral.state != CBPeripheralStateConnected) {
            @synchronized (mgr.endpoints) {
                [mgr.endpoints removeObjectForKey:nsuuid.UUIDString];
            }
            [mgr.central cancelPeripheralConnection:peripheral];
            return NULL;
        }

        CBUUID *hidppUUID = [CBUUID UUIDWithString:kLogitechHidppServiceUUID];
        CBUUID *devInfoUUID = [CBUUID UUIDWithString:kDeviceInfoServiceUUID];
        [peripheral discoverServices:@[hidppUUID, devInfoUUID]];
        if (!wait_ms(ep.serviceSem, 5000) || ep.disconnected) {
            @synchronized (mgr.endpoints) {
                [mgr.endpoints removeObjectForKey:nsuuid.UUIDString];
            }
            [mgr.central cancelPeripheralConnection:peripheral];
            return NULL;
        }

        CBService *hidppService = nil;
        CBService *devInfoService = nil;
        for (CBService *s in peripheral.services) {
            if ([s.UUID isEqual:hidppUUID]) {
                hidppService = s;
            } else if ([s.UUID isEqual:devInfoUUID]) {
                devInfoService = s;
            }
        }
        if (!hidppService) {
            @synchronized (mgr.nonLogitech) {
                [mgr.nonLogitech addObject:nsuuid.UUIDString];
            }
            @synchronized (mgr.endpoints) {
                [mgr.endpoints removeObjectForKey:nsuuid.UUIDString];
            }
            [mgr.central cancelPeripheralConnection:peripheral];
            return NULL;
        }

        ep.pendingCharServices = devInfoService ? 2 : 1;
        [peripheral discoverCharacteristics:nil forService:hidppService];
        if (devInfoService) {
            [peripheral discoverCharacteristics:@[[CBUUID UUIDWithString:kPnpIdCharUUID]] forService:devInfoService];
        }
        if (!wait_ms(ep.charSem, 5000) || !ep.hidppChar || ep.disconnected) {
            @synchronized (mgr.endpoints) {
                [mgr.endpoints removeObjectForKey:nsuuid.UUIDString];
            }
            [mgr.central cancelPeripheralConnection:peripheral];
            return NULL;
        }

        if (ep.pnpChar) {
            [peripheral readValueForCharacteristic:ep.pnpChar];
            wait_ms(ep.pnpSem, 1500);
        }

        [peripheral setNotifyValue:YES forCharacteristic:ep.hidppChar];
        if (!wait_ms(ep.notifySem, 3000) || !ep.notifyOk || ep.disconnected) {
            @synchronized (mgr.endpoints) {
                [mgr.endpoints removeObjectForKey:nsuuid.UUIDString];
            }
            [mgr.central cancelPeripheralConnection:peripheral];
            return NULL;
        }

        NSString *pname = peripheral.name ?: @"Logitech Bluetooth Device";
        if (name_out && name_cap > 0) {
            strncpy(name_out, pname.UTF8String ?: "", name_cap - 1);
            name_out[name_cap - 1] = '\0';
        }
        if (product_id_out) {
            *product_id_out = ep.productId;
        }
        return (__bridge_retained void *)ep;
    }
}

bool fs_ble_write(void *handle, const uint8_t *data, size_t len) {
    if (!handle || !data || len == 0) return false;
    @autoreleasepool {
        FsBleEndpoint *ep = (__bridge FsBleEndpoint *)handle;
        if (ep.closed || ep.disconnected || ep.peripheral.state != CBPeripheralStateConnected || !ep.hidppChar) {
            ep.disconnected = true;
            return false;
        }
        NSData *payload = [NSData dataWithBytes:data length:len];
        CBCharacteristicProperties props = ep.hidppChar.properties;
        if (props & CBCharacteristicPropertyWrite) {
            while (dispatch_semaphore_wait(ep.writeSem, DISPATCH_TIME_NOW) == 0) {}
            ep.writeOk = false;
            [ep.peripheral writeValue:payload forCharacteristic:ep.hidppChar type:CBCharacteristicWriteWithResponse];
            if (!wait_ms(ep.writeSem, 1500) || !ep.writeOk) {
                return false;
            }
            return true;
        } else {
            [ep.peripheral writeValue:payload forCharacteristic:ep.hidppChar type:CBCharacteristicWriteWithoutResponse];
            return true;
        }
    }
}

// Returns >0 bytes read, 0 on timeout, -1 on disconnect.
int fs_ble_read(void *handle, uint8_t *out_buf, size_t cap, int32_t timeout_ms) {
    if (!handle || !out_buf || cap == 0) return -1;
    @autoreleasepool {
        FsBleEndpoint *ep = (__bridge FsBleEndpoint *)handle;
        if (ep.closed || ep.disconnected || ep.peripheral.state != CBPeripheralStateConnected) {
            ep.disconnected = true;
            return -1;
        }
        NSData *item = nil;
        @synchronized (ep.reports) {
            if (ep.reports.count > 0) {
                item = ep.reports.firstObject;
                [ep.reports removeObjectAtIndex:0];
            }
        }
        if (!item && timeout_ms > 0) {
            wait_ms(ep.readSem, timeout_ms);
            if (ep.closed || ep.disconnected || ep.peripheral.state != CBPeripheralStateConnected) {
                ep.disconnected = true;
                return -1;
            }
            @synchronized (ep.reports) {
                if (ep.reports.count > 0) {
                    item = ep.reports.firstObject;
                    [ep.reports removeObjectAtIndex:0];
                }
            }
        }
        if (!item) {
            return 0;
        }
        size_t n = item.length < cap ? item.length : cap;
        memcpy(out_buf, item.bytes, n);
        return (int)n;
    }
}
