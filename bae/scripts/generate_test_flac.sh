#!/bin/bash
# Throwaway script to generate test FLAC fixtures
# Uses ffmpeg or sox to create simple sine wave audio files

OUTPUT_DIR="tests/fixtures/flac"
mkdir -p "$OUTPUT_DIR"

SAMPLE_RATE=44100
DURATION=5  # 5 seconds - should be around 70-100KB compressed
FREQ1=440
FREQ2=660

if command -v ffmpeg &> /dev/null; then
    echo "Using ffmpeg to generate FLAC files..."
    ffmpeg -f lavfi -i "sine=frequency=$FREQ1:duration=$DURATION" \
           -ar $SAMPLE_RATE -ac 1 -sample_fmt s16 -f flac \
           "$OUTPUT_DIR/01 Test Track 1.flac" -y -loglevel error
    
    ffmpeg -f lavfi -i "sine=frequency=$FREQ2:duration=$DURATION" \
           -ar $SAMPLE_RATE -ac 1 -sample_fmt s16 -f flac \
           "$OUTPUT_DIR/02 Test Track 2.flac" -y -loglevel error
elif command -v sox &> /dev/null; then
    echo "Using sox to generate FLAC files..."
    sox -n -r $SAMPLE_RATE -c 1 "$OUTPUT_DIR/01 Test Track 1.flac" \
        synth $DURATION sine $FREQ1
    sox -n -r $SAMPLE_RATE -c 1 "$OUTPUT_DIR/02 Test Track 2.flac" \
        synth $DURATION sine $FREQ2
else
    echo "Error: Neither ffmpeg nor sox is available"
    echo "Install one: brew install ffmpeg (or sox)"
    exit 1
fi

echo "Generated FLAC fixtures in $OUTPUT_DIR"
ls -lh "$OUTPUT_DIR"

