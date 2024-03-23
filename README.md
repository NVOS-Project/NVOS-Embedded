# NVOS-Embedded
This repository contains the embedded firmware that drives the NVOS root controller.

# Goals:
 - Dynamic deployment of the NVOS platform
 - Reliable and safe driver for various electronics
 - Allows for quick prototyping and deployment of new NVOS hardware APIs
 - Exposes APIs used for managing the NVOS platform

# Implementation status and planned features:
 - ### System
   - Exclusive GPIO access layer: ✔️
   - Persistent ADB server access API: ✔️
   - Networking failure recovery: ✔️
   - Device server: ✔️
   - gRPC server: ✔️
   - Device capability API (for building stable gRPC APIs): ✔️
   - Configuration file: ✔️
   - Configuration hot-reload: ❌
   - Dynamic bus controller loading (on startup): ✔️
   - Dynamic device driver loading (any time): ✔️ (supported, but hot reload capability is not exposed to clients)
- ### Controllers
  - #### Raw GPIO pin access:
    - raw: ✔️ (Not supported on our hardware)
    - raw_sysfs: ✔️
  - #### PWM access:
    - pwm: ✔️ (Not supported on our hardware)
    - pwm_sysfs: ✔️
  - #### I2C access:
    - i2c: ✔️ (Not supported on our hardware)
    - i2c_sysfs: ✔️
  - #### UART access:
    - uart: ✔️
- ### RPC services / Capabilities:
  - Device reflection: ✔️
  - Networking: ✔️
  - GPS: ✔️
  - Compass: ❌
  - LED: ✔️
  - Light sensor: ✔️
  - Thermometer: ✔️
  - Barometer:  ✔️
- ### Device drivers:
  - LED (sysfs_generic_led): ✔️
  - GPS (gps_uart): ✔️
  - Compass (???): ❌
  - Ambient light sensor (tsl2591_sysfs):✔️
  - Temperature (bmp280_sysfs): ✔️
