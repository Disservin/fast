import { createApp } from "vue";

import App from "./App.vue";
import router from "./router";
import VueApexCharts from "vue3-apexcharts";
import FontAwesomeIcon from "./fontawesome-icons";

// Vuetify
import "vuetify/styles";
import { createVuetify } from "vuetify";
import * as components from "vuetify/components";
import * as directives from "vuetify/directives";

const vuetify = createVuetify({
  components,
  directives,
});

createApp(App)
  .use(router)
  .use(vuetify)
  .use(VueApexCharts)
  .component("FontAwesomeIcon", FontAwesomeIcon)
  .mount("#app");
