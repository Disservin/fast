import Vue from "vue";

import App from "./App.vue";
import router from "./router";

import FontAwesomeIcon from "./fontawesome-icons";

import chessboard from 'vue-chessboard';

Vue.use(chessboard);

Vue.component("font-awesome-icon", FontAwesomeIcon);

new Vue({
  router,
  render: (h) => h(App),
})
.$mount("#app");

