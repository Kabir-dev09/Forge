#pragma once
#include <FL/Fl_Group.H>
#include <FL/Fl_Progress.H>
#include <FL/Fl_Box.H>
#include <FL/Fl_Button.H>
#include <thread>
#include <atomic>
#include "../core/installer_state.h"

namespace gui {

class MainWindow;

class StepProgress : public Fl_Group {
public:
    StepProgress(int x, int y, int w, int h, MainWindow* parent_window, core::InstallerState& state);
    ~StepProgress();
    
private:
    MainWindow* main_window;
    core::InstallerState& install_state;
    
    Fl_Box* status_label;
    Fl_Progress* progress_bar;
    Fl_Button* btn_finish;
    
    std::thread worker_thread;
    std::atomic<bool> is_running;
    
    static void update_ui_cb(void* data);
    static void cb_finish(Fl_Widget* w, void* data);
    
    void run_installation();
    
    void set_status(const std::string& msg, float progress);
    void finish_with_error(const std::string& err);
    void finish_with_success();

    struct ProgressMessage {
        StepProgress* self;
        std::string text;
        float progress;
        bool is_error;
        bool is_done;
    };
};

} // namespace gui
