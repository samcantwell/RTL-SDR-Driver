# RTL-SDR Driver

This program will output recordings from an RTL-SDR.

## Will 

- Connect to an RTL-SDRv3 with tuner R860
- Connect from a MacBook Air M4 running the latest OS
- Record for 10 seconds on the specified frequency
- Output that recording as a file containing raw unsigned 8-bit IQ samples
- Output recording metadata (frequency, rate, date/time) in the file name
- 'Drive' the SDR from scratch (will not rely on other drivers/software)

## Will not (for now)

- Connect to other SDRs or RTL-SDR models
- Connect from any other computer or software
- Output in any other format
- Display the data in any way
- Process or demodulate the data in any way
- Expose an API as a library to be used by other programs
- Rely on RTL-SDR specific drivers
