namespace beta {

struct Runner {
    void run();
};

void Runner::run() {}

}  // namespace beta

void run_all() {
    beta::Runner runner;
    runner.run();
}
