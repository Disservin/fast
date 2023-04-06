import { createApp } from "vue";

import App from "./App.vue";
import router from "./router";

import FontAwesomeIcon from "./fontawesome-icons";

createApp(App)
    .use(router)
    .component("FontAwesomeIcon", FontAwesomeIcon)
    .mount('#app')
