#pragma once

#include <string>
#include <vector>

namespace ange {

// Forward declaration only.  The definition lives in src/ecs.cpp.
void validate_param(const std::string& name, int value);

class EcsWorld {
public:
    EcsWorld();
    ~EcsWorld();

    void run();
    int entity_count() const;

private:
    std::vector<int> entities_;
};

}  // namespace ange
