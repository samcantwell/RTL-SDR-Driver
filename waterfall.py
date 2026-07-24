#!/usr/bin/env python3
"""Waterfall plot of a raw RTL-SDR IQ recording (interleaved unsigned 8-bit)."""

import sys

import numpy as np
import matplotlib.pyplot as plt

SAMPLE_RATE = 2.048e6   # Hz
CENTRE_FREQ = 101.9e6   # Hz
FFT_SIZE = 2048

path = sys.argv[1] if len(sys.argv) > 1 else "output.bin"

raw = np.fromfile(path, dtype=np.uint8)
# u8 IQ: zero point is ~127.4, scale to roughly [-1, 1)
iq = (raw.astype(np.float32) - 127.4) / 128.0
samples = iq[0::2] + 1j * iq[1::2]

duration = len(samples) / SAMPLE_RATE
print(f"{len(samples)} complex samples = {duration:.2f} s at {SAMPLE_RATE/1e6} Msps")

# Chop into FFT_SIZE rows, window each, FFT, and shift DC to the centre
n_rows = len(samples) // FFT_SIZE
frames = samples[: n_rows * FFT_SIZE].reshape(n_rows, FFT_SIZE)
window = np.hanning(FFT_SIZE)
spectra = np.fft.fftshift(np.fft.fft(frames * window, axis=1), axes=1)
power_db = 20 * np.log10(np.abs(spectra) + 1e-10)

freqs_mhz = (CENTRE_FREQ + np.fft.fftshift(np.fft.fftfreq(FFT_SIZE, 1 / SAMPLE_RATE))) / 1e6

plt.figure(figsize=(12, 8))
plt.imshow(
    power_db,
    aspect="auto",
    extent=[freqs_mhz[0], freqs_mhz[-1], duration, 0],
    cmap="viridis",
)
plt.xlabel("Frequency (MHz)")
plt.ylabel("Time (s)")
plt.title(f"Waterfall: {path} @ {CENTRE_FREQ/1e6} MHz")
plt.colorbar(label="Power (dB)")
plt.tight_layout()
plt.show()
