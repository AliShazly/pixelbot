# pixelbot

Aims for you in FPS games by detecting colored outlines around enemies.

**Don't use this online**

![app screenshot](https://user-images.githubusercontent.com/44154670/166622472-0d3c2219-7e9c-4850-82e9-46c00ed7039c.png)

## Building

`cargo build --release`

## Features

-   Fast game capture using the native Windows Desktop Duplication API
-   Works on any game with outlined characters
-   Target clustering for when more than one enemy is on screen
-   Auto clicker
-   Configurable aim

## Features???

-   AVX2 alpha blending
-   Real-time FPS graph
-   Custom config file parser
-   _pretty colors_ ![colors](https://user-images.githubusercontent.com/44154670/166624216-2c990bc4-fa2b-404b-be61-0ba7654d36e2.gif)
