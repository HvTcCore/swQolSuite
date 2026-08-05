# Attention, this is an [unofficial] repository of the program!

A copy of the repository was created in case the original repository was deleted, as well as for the probable support of the program in a working state

### The original repository (archived) is located here: https://github.com/PieKing1215/swQolSuite

# swQolSuite
Mod for [Stormworks: Build and Rescue](https://store.steampowered.com/app/573090/Stormworks_Build_and_Rescue/) that adds some QOL settings.<br/>
Only supports 64 bit Windows version (stormworks64.exe). It might work through wine/proton/etc, but untested.
#### Check [`THIS`](https://github.com/HvTcCore/swQolSuite/wiki/HowTo-run-on-Linux-in-Steam) wiki page to see how to run on Linux in Steam:

<img src="https://github.com/user-attachments/assets/cdff3e95-1c6f-4c14-9a02-fffd649d425f" style="width:40%" />


## SWRIPE integrated xml functions
This [[Xenograft](https://github.com/HvTcCore/swQolSuite/tree/xenograft)] branch created to test ported&integrated property xml functions from [SWRIPE](https://github.com/MaxPain09/Stormworks-Realtime-Illegal-Property-Editor) [Link to original repo].<br/>
**!It can be more unstable than [[main](https://github.com/HvTcCore/swQolSuite/tree/main)] SwQolSuite branch, so use at your own risk!**<br/>
Also this version required to tests, so be free to write in [issues](https://github.com/HvTcCore/swQolSuite/issues) with problems with it.<br/>
### Original SWRIPE author github profile - https://github.com/MaxPain09<br/>

<img width="322" height="583" alt="image" src="https://github.com/user-attachments/assets/a689975e-dfb8-479b-a960-ad291f38a24b" />


## READ THIS FIRST
swQolSuite uses low level techniques (DLL injection + ASM manipulation) to patch the game since there's no other way to do it.<br/>
Due to the nature of these methods, I won't make any guarantees about stability.<br/>
**If something goes wrong it will most likely crash your game, so please save your work often.**<br/>
Injecting or ejecting the mod are the most likely to crash, but it might also be possible when changing settings while in use (eg. changing editor settings while in editor)<br/>

**swQolSuite will likely be flagged by your Windows/antiviruses** (`Trojan:Win32/Wacatac.B!ml`) as a side effect of how it works. The way swQolSuite injects into the game is similar to how some viruses hook other processes, and some antiviruses will detect this.
Obviously I will say original repo (PieKing1215/swQolSuite) or unofficial repo (HvTcCore/swQolSuite) and official/unofficial builds do not actually contain malware to my knowledge, but if you want to be sure, look over the code and [build from source](https://github.com/HvTcCore/swQolSuite/wiki/Building) **[unofficial wiki]**.

swQolSuite is not a cheating/griefing tool, please do not request features that give you an advantage over other players in multiplayer.

## Download
For releases, see [Releases](../../releases).<br/>
For dev builds: sign in to GitHub, go [here](https://github.com/HvTcCore/swQolSuite/actions/workflows/autobuild.yml?query=branch%3Axenograft+is%3Asuccess), click the latest one, scroll down to "Artifacts" and download it.<br/>
Or download the latest at https://nightly.link/HvTcCore/swQolSuite/workflows/autobuild/xenograft<br/>
Unzip and run swqols-inject.exe to run.

## Basic Usage
Have Stormworks open and run the injector exe.<br/>
A couple menus should appear ingame:
- swQolSuite: shows version number & commit SHA, and has an "Eject" button which removes the mod.<br/>
- Tweaks: shows the settings for all of the mod's features, see below for details.<br/>
- Errors: if there were any errors loading features, they will be shown in this window (hidden if no errors).<br/>

*Note: swQolSuite does not modify any Stormworks files on disk, so if you close & reopen the game you have to re-run the injector exe*

You can press the grave/backtick/tilde key [`` `~ ``] to toggle the visibility of the menus.

## Game Updates
The patches are set up so they should usually continue to work after game updates unless related parts of code were touched.<br/>
If after an update any patches fail, you should just get an error in the error window and the failing tweak will be disabled until it is updated to work again.

# Features
See [Tweaks & Settings](https://github.com/HvTcCore/swQolSuite/wiki/Tweaks-&-Settings) **[unofficial wiki]**

## Licenses

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

## Disclaimer
I am not personally affiliated with Stormworks: Build and Rescue or Geometa, nor has Stormworks: Build and Rescue or Geometa endorsed this product.<br/>
Stormworks: Build and Rescue and any of its content or materials are the property of their respective owners.
