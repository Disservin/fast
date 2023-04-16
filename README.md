# Fast

a hopefully more modern chess gui

![Interface](./src/assets/imgs/interface.jpeg?)

## A few notes

This project is currently **WIP** and certain features might be missing  
from the releases or not work at all.

If you think something is a bug you can open an issue, similarly for feature requests.

Since this is also my first project with vue and tauri some parts of the code  
might not be as clean as they could be ; ) Feel free to improve these areas.

## Usability

These keybindings are mapped globally:

```
CTRL + W = Close Window
```

These keybindings are mapped in the Analysis Vue:

```
CTRL + G = Engine Go Infinite
CTRL + H = Engine Halt
CTRL + R = Engine Restart
CTRL + N = New Board

ArrowLeft = Undo Move
```

## Requirements

Make sure that the tauri submodule is properly cloned as well.

Why do you have a submodule of tauri ?

Tauri doesnt allow arbitrary commands to be executed. Of course  
we would want to support any chess engine so we need to bypass this limitation.  
That's where the submodule comes in.

```
git submodule update --init --recursive
```

## Contributing

Any contributions are welcome, if you add vue.js code try to follow their style guide.
If you can implement something with [Vuetify](https://vuetifyjs.com/en/) prefer it over implementing it again from scratch ; )

I also use [prettier](https://github.com/prettier/prettier) to format the code.

Cleanup pr's are also very welcome since this is my first vue/tauri project.

## Project Setup

Follow the prerequisites for tauri from here:
https://tauri.app/v1/guides/getting-started/prerequisites/

If you encounter issues with npm first check your node version and update  
to a later version. I recommend [nvm](https://github.com/nvm-sh/nvm) to manage multiple verison.

If you still have issues try to delete the `node_modules` as well as the `package-lock.json`  
and rerun the npm install.

You also might have to create a root level `dist` folder.

```sh
npm install
```

### Compile and Hot-Reload for Development

```sh
npm run tauri dev
```

### Type-Check, Compile and Minify for Production

```sh
npm run tauri build
```
