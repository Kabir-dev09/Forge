#include <FL/Fl.H>
#include <unistd.h>
#include <iostream>
#include <curl/curl.h>
#include "gui/main_window.h"

int main(int argc, char **argv) {
    curl_global_init(CURL_GLOBAL_DEFAULT);
    Fl::lock(); // Enable thread safety for Fl::awake()
    
    Fl::scheme("gtk+");
    Fl::background(255, 255, 255); // Global white background for a clean look
    
    gui::MainWindow* window = new gui::MainWindow();
    window->show(argc, argv);
    
    int ret = Fl::run();
    curl_global_cleanup();
    return ret;
}
