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
   - Networking failure recovery: ❌
   - Device server: ✔️
   - gRPC server: ✔️
   - Device capability API (for building stable gRPC APIs): ✔️
   - Configuration file: ✔️
   - Configuration hot-reload: ❌
   - Dynamic bus controller loading (on startup): ✔️
   - Dynamic device driver loading (any time): ✔️
- ### Controllers
  - #### Raw GPIO pin access:
    - raw: ✔️ (BROKEN)
    - raw_sysfs: ✔️
  - #### PWM access:
    - pwm: ✔️ (BROKEN)
    - pwm_sysfs: ✔️
  - #### I2C access:
    - i2c: ✔️ (BROKEN)
    - i2c_sysfs: ✔️
  - #### UART access:
    - uart: ✔️
- ### RPC services / Capabilities:
  - Device reflection: ✔️
  - Networking: ❌
  - GPS: ❌
  - Compass: ❌
  - Lighting: ❌
- ### Device drivers:
  - LED (???): ❌
  - GPS (gps_uart): ❌
  - Compass (???): ❌
