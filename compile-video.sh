#!/bin/bash
ffmpeg -r 60 -f image2 -s 1280x720 -i frames/%06d.png -vcodec libx264 -pix_fmt yuv420p -crf 0 -q:v 2 output.mp4
